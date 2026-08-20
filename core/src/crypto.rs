//! Every message is sealed before it leaves the machine.
//!
//! A channel's key is 32 bytes of real entropy, made by the app and never
//! typed by anyone, so it is used as the key directly. The previous scheme
//! stretched five human-chosen words through Argon2, which was the right thing
//! to do with about thirty bits of entropy and is simply unnecessary now.
//!
//! The cipher is XChaCha20-Poly1305: it authenticates as well as encrypts, so a
//! frame that has been tampered with does not open at all rather than opening
//! into something subtly wrong. Its 24-byte nonces are large enough to pick at
//! random without ever worrying about a collision.
//!
//! Every frame is bound to the connection it belongs to. The server opens by
//! sending a fresh random challenge, and that challenge is the associated data
//! for every frame afterwards — so a frame captured from an earlier connection
//! cannot be replayed into a later one. It will not open.
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use std::io;

pub const CHALLENGE_LEN: usize = 32;
const NONCE_LEN: usize = 24;

pub fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

/// Seals and opens frames for one connection on one channel.
pub struct Sealer {
    cipher: XChaCha20Poly1305,
    aad: Vec<u8>,
}

impl Sealer {
    pub fn new(key: &[u8], challenge: &[u8]) -> Option<Sealer> {
        if key.len() != 32 {
            return None;
        }
        Some(Sealer {
            cipher: XChaCha20Poly1305::new(key.into()),
            aad: challenge.to_vec(),
        })
    }

    pub fn seal(&self, plain: &[u8]) -> String {
        let nonce_bytes = random(NONCE_LEN);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(nonce, Payload { msg: plain, aad: &self.aad })
            .expect("encrypt");
        let mut frame = nonce_bytes;
        frame.extend_from_slice(&ct);
        B64.encode(frame)
    }

    pub fn open(&self, frame: &str) -> io::Result<Vec<u8>> {
        let raw = B64
            .decode(frame.trim())
            .map_err(|_| io::Error::other("frame is not valid base64"))?;
        if raw.len() < NONCE_LEN + 16 {
            return Err(io::Error::other("frame too short"));
        }
        let (nonce, ct) = raw.split_at(NONCE_LEN);
        self.cipher
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: &self.aad })
            .map_err(|_| io::Error::other("that frame does not open with this channel's key"))
    }
}
