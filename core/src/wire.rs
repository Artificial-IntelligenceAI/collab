//! The connection: a challenge in the clear, then nothing else in the clear.
use crate::config;
use crate::crypto::{random, Sealer, CHALLENGE_LEN};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

pub const PROTOCOL: u32 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hello {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub since: i64,
    /// "watch" | "post" | "fetch"
    #[serde(default)]
    pub mode: String,
}

/// Sent back, sealed, once a Hello has actually opened. Its whole job is to
/// prove the far side holds the same key: without it a client seals a message
/// with the wrong word, writes it into the socket, and the write succeeds —
/// so a message nobody could read would look exactly like one delivered.
#[derive(Debug, Serialize, Deserialize)]
pub struct Welcome {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub server: String,
}

#[derive(Serialize, Deserialize)]
struct Greeting {
    collab: u32,
    challenge: String,
}

/// A sealed connection. Every read and write past the greeting is encrypted.
pub struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    sealer: Sealer,
}

fn need_key() -> io::Error {
    io::Error::other(
        "no shared key set — run `collab key -new` here, then copy the line it prints \
         into ~/.collab-config on the other machine",
    )
}

impl Conn {
    /// Server side: state a challenge, then expect everything sealed against it.
    pub fn accept(stream: TcpStream) -> io::Result<Conn> {
        let word = config::key().ok_or_else(need_key)?;
        let challenge = random(CHALLENGE_LEN);
        let mut writer = stream.try_clone()?;
        let greeting = serde_json::to_string(&Greeting {
            collab: PROTOCOL,
            challenge: B64.encode(&challenge),
        })?;
        writeln!(writer, "{greeting}")?;
        writer.flush()?;
        Ok(Conn {
            reader: BufReader::new(stream),
            writer,
            sealer: Sealer::new(&word, &challenge),
        })
    }

    /// Client side: take the challenge, then seal everything against it.
    pub fn connect(stream: TcpStream) -> io::Result<Conn> {
        let word = config::key().ok_or_else(need_key)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::other("server hung up before saying anything"));
        }
        let greeting: Greeting = serde_json::from_str(line.trim())
            .map_err(|_| io::Error::other("this does not look like a collab server"))?;
        if greeting.collab != PROTOCOL {
            return Err(io::Error::other(format!(
                "the other machine speaks collab v{} and this one speaks v{PROTOCOL} — \
                 update the older one",
                greeting.collab
            )));
        }
        let challenge = B64
            .decode(&greeting.challenge)
            .map_err(|_| io::Error::other("bad challenge"))?;
        Ok(Conn {
            reader,
            writer: stream,
            sealer: Sealer::new(&word, &challenge),
        })
    }

    /// Waits for the far side to prove it holds the same key.
    pub fn expect_welcome(&mut self) -> io::Result<Welcome> {
        match self.recv::<Welcome>() {
            Ok(Some(w)) if w.ok => Ok(w),
            Ok(_) => Err(io::Error::other(
                "the other machine hung up without answering — its `key` in ~/.collab-config \
                 does not match this one",
            )),
            Err(e) => Err(e),
        }
    }

    pub fn send<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        let plain = serde_json::to_vec(value)?;
        writeln!(self.writer, "{}", self.sealer.seal(&plain))?;
        self.writer.flush()
    }

    /// None at end of stream. An error means the frame would not open, which is
    /// either the wrong key or someone meddling — either way, not a message.
    pub fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.trim().is_empty() {
            return Ok(None);
        }
        let plain = self.sealer.open(&line)?;
        Ok(Some(serde_json::from_slice(&plain)?))
    }
}
