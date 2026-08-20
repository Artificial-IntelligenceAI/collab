//! Every message is sealed before it leaves the machine.
//!
//! One shared word, the same on both machines, becomes a real key through
//! Argon2id — a short word straight into a cipher would be guessable, and the
//! whole point is that "sis's AI edited ShopHandler" cannot be forged by
//! anything else on the Wi-Fi.
//!
//! The cipher is XChaCha20-Poly1305: it authenticates as well as encrypts, so a
//! frame that has been tampered with does not decrypt at all rather than
//! decrypting into something subtly wrong. Its 24-byte nonces are large enough
//! to pick at random without ever worrying about a collision.
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

const SALT: &[u8] = b"collab.key.derivation.v3";
pub const CHALLENGE_LEN: usize = 32;
const NONCE_LEN: usize = 24;

pub fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

/// A fresh key nobody has to invent. Words, because it gets copied by hand
/// between two machines by someone who is not a typist.
pub fn new_key() -> String {
    const WORDS: [&str; 64] = [
        "amber", "anchor", "badger", "basalt", "beacon", "birch", "bramble", "bronze", "cactus",
        "canyon", "cedar", "cinder", "clover", "cobalt", "comet", "copper", "coral", "cypress",
        "dahlia", "delta", "ember", "fathom", "fennel", "flint", "garnet", "gecko", "glacier",
        "granite", "harbor", "heron", "indigo", "ivory", "jasper", "juniper", "kelp", "lantern",
        "lichen", "marble", "meadow", "mesa", "nectar", "nimbus", "oakum", "obsidian", "onyx",
        "opal", "pebble", "pewter", "quartz", "quill", "ravine", "rowan", "saffron", "sable",
        "thistle", "tundra", "umber", "valley", "velvet", "walnut", "willow", "yarrow", "zephyr",
        "zinc",
    ];
    let mut rng = rand::thread_rng();
    // 5 words from 64 is 30 bits; Argon2id is what makes that expensive to guess.
    (0..5)
        .map(|_| WORDS[(rng.next_u32() as usize) % WORDS.len()])
        .collect::<Vec<_>>()
        .join("-")
}

fn derive(word: &str) -> [u8; 32] {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(19 * 1024, 2, 1, Some(32)).expect("argon2 params");
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(word.trim().as_bytes(), SALT, &mut out)
        .expect("argon2");
    out
}

/// Seals and opens frames for one connection.
pub struct Sealer {
    cipher: XChaCha20Poly1305,
    aad: Vec<u8>,
}

impl Sealer {
    pub fn new(word: &str, challenge: &[u8]) -> Self {
        Sealer {
            cipher: XChaCha20Poly1305::new(&derive(word).into()),
            aad: challenge.to_vec(),
        }
    }

    pub fn seal(&self, plain: &[u8]) -> String {
        let nonce_bytes = random(NONCE_LEN);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plain,
                    aad: &self.aad,
                },
            )
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
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ct,
                    aad: &self.aad,
                },
            )
            .map_err(|_| {
                io::Error::other(
                    "could not open the message — the other machine's key does not match",
                )
            })
    }
}
