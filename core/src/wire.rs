//! The connection: a challenge in the clear, then nothing else in the clear.
//!
//! A connection belongs to exactly one channel. The client seals its Hello with
//! that channel's key; the server works out which channel by trying the keys it
//! holds until one opens the frame. That is cheap now that a key is 32 random
//! bytes rather than something stretched from words — and it means the channel
//! name never travels in the clear, so the wire does not even reveal what the
//! two of you are working on.
use crate::channels;
use crate::crypto::{random, Sealer, CHALLENGE_LEN};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub const PROTOCOL: u32 = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hello {
    /// The display name to post under — an AI's chosen name, or the machine's.
    #[serde(default)]
    pub name: String,
    /// The machine, always, whatever `name` says.
    #[serde(default)]
    pub host: String,
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
/// with the wrong one, writes it into the socket, and the write succeeds — so a
/// message nobody could read would look exactly like one delivered.
#[derive(Debug, Serialize, Deserialize)]
pub struct Welcome {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub server: String,
    /// Which machine made this channel. Someone who was handed a key does not
    /// otherwise know, and needs to, to be told who can close the room.
    #[serde(default)]
    pub creator: String,
    /// How far the channel had got when this connection opened. Everything at
    /// or below it that arrives afterwards is backlog, everything above is
    /// live. Without it the two are the same frame, and a two-hour-old
    /// instruction is delivered looking exactly like one just given.
    #[serde(default)]
    pub head: i64,
}

/// What a file transfer announces before its bytes.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileHeader {
    #[serde(default)]
    pub file: crate::files::FileRef,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub via: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Want {
    #[serde(default)]
    pub hash: String,
}

/// The answer to something that either happened or did not.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ack {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub detail: String,
}

#[derive(Serialize, Deserialize)]
struct Greeting {
    collab: u32,
    challenge: String,
}

pub struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    sealer: Sealer,
}

fn no_channel(name: &str) -> io::Error {
    io::Error::other(format!(
        "no key for #{name} on this machine — channels are made in the collab app, \
         and the key has to be copied to every machine that uses them"
    ))
}

impl Conn {
    /// Server side: state a challenge, then find which channel's key opens what
    /// comes back. A frame that opens with none of them is not a message.
    pub fn accept(stream: TcpStream) -> io::Result<(Conn, String, Hello)> {
        let challenge = random(CHALLENGE_LEN);
        let mut writer = stream.try_clone()?;
        let greeting = serde_json::to_string(&Greeting {
            collab: PROTOCOL,
            challenge: B64.encode(&challenge),
        })?;
        writeln!(writer, "{greeting}")?;
        writer.flush()?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::other("client hung up"));
        }

        for (name, ch) in channels::load() {
            let Some(key) = B64.decode(ch.key.trim()).ok().filter(|k| k.len() == 32) else {
                continue;
            };
            let Some(sealer) = Sealer::new(&key, &challenge) else {
                continue;
            };
            let Ok(plain) = sealer.open(&line) else {
                continue;
            };
            let Ok(hello) = serde_json::from_slice::<Hello>(&plain) else {
                continue;
            };
            return Ok((
                Conn {
                    reader,
                    writer,
                    sealer,
                },
                name,
                hello,
            ));
        }
        Err(io::Error::other("no channel key opened that"))
    }

    /// Client side: take the challenge, then seal everything with the key for
    /// the channel being joined.
    pub fn connect(stream: TcpStream, channel: &str) -> io::Result<Conn> {
        let key = channels::key_bytes(channel).ok_or_else(|| no_channel(channel))?;
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
        let sealer = Sealer::new(&key, &challenge)
            .ok_or_else(|| io::Error::other("that channel's key is the wrong length"))?;
        Ok(Conn {
            reader,
            writer: stream,
            sealer,
        })
    }

    /// Waits for the far side to prove it holds the same key.
    pub fn expect_welcome(&mut self) -> io::Result<Welcome> {
        match self.recv::<Welcome>() {
            Ok(Some(w)) if w.ok => Ok(w),
            Ok(_) => Err(io::Error::other(
                "the other machine hung up without answering — it does not have this \
                 channel's key, or does not know the channel at all",
            )),
            Err(e) => Err(e),
        }
    }

    /// A short read timeout is how a watcher notices that its chat has joined a
    /// different channel while this one is quiet, without polling anything.
    pub fn set_read_timeout(&mut self, d: Option<std::time::Duration>) {
        let _ = self.reader.get_ref().set_read_timeout(d);
    }

    /// Raw bytes, sealed. File chunks go this way rather than as JSON: base64
    /// inside JSON inside a base64 frame would carry the file nearly twice
    /// over, where this carries it about a third over.
    pub fn send_raw(&mut self, data: &[u8]) -> io::Result<()> {
        writeln!(self.writer, "{}", self.sealer.seal(data))?;
        self.writer.flush()
    }

    /// None at end of stream; an empty vector is the sender saying "that's all".
    pub fn recv_raw(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.read_frame()? {
            None => Ok(None),
            Some(line) => Ok(Some(self.sealer.open(&line)?)),
        }
    }

    /// A frame nobody should be sending is not read into memory just because
    /// somebody asked. The cap is a comfortable multiple of the largest frame
    /// this protocol produces, which is one file chunk.
    fn read_frame(&mut self) -> io::Result<Option<String>> {
        const MAX_FRAME: usize = 2 * 1024 * 1024;
        let mut line = String::new();
        let mut limited = (&mut self.reader).take(MAX_FRAME as u64);
        if limited.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if !line.ends_with('\n') && line.len() >= MAX_FRAME {
            return Err(io::Error::other("frame too large"));
        }
        if line.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(line))
    }

    pub fn send<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        let plain = serde_json::to_vec(value)?;
        writeln!(self.writer, "{}", self.sealer.seal(&plain))?;
        self.writer.flush()
    }

    /// None at end of stream. An error means the frame would not open, which is
    /// either the wrong key or someone meddling — either way, not a message.
    pub fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        let Some(line) = self.read_frame()? else {
            return Ok(None);
        };
        let plain = self.sealer.open(&line)?;
        Ok(Some(serde_json::from_slice(&plain)?))
    }
}
