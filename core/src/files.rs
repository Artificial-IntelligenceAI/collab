//! Files, kept by content rather than by name.
//!
//! A message carries only a reference — name, size, hash — and the bytes live
//! in a store beside the history. Putting them in the message itself would mean
//! a screenshot replayed to every watcher on every reconnect, sitting in the
//! history for ever, and landing on people who never asked for it.
//!
//! The hash is the identity. That is what makes it safe to trust a file the
//! other machine sent: the name is whatever the sender typed, but the bytes
//! either hash to what the message claimed or they are not the file.
use crate::config;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Big enough for a Roblox model or a screenshot, small enough that a mistake
/// does not wedge the machine. Sealed frames are base64, so the wire carries
/// about a third more than this.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FileRef {
    /// What the sender called it. Never used as a path without cleaning first.
    pub name: String,
    pub size: u64,
    /// sha256, hex. The file's real name, as far as the store is concerned.
    pub hash: String,
}

pub fn store_dir(channel: &str) -> PathBuf {
    config::home(".collab-files").join(safe_component(channel))
}

pub fn blob_path(channel: &str, hash: &str) -> PathBuf {
    store_dir(channel).join(safe_component(hash))
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// One path element, with anything that could climb out of the directory
/// removed. A sender chooses the name, so it is not to be trusted with a path.
pub fn safe_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "unnamed".into()
    } else {
        cleaned.chars().take(120).collect()
    }
}

/// Where a received file goes, without ever overwriting something already there.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let name = safe_component(name);
    let mut candidate = dir.join(&name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 2..1000 {
        candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate
}

pub fn save_blob(channel: &str, hash: &str, data: &[u8]) -> std::io::Result<()> {
    let dir = store_dir(channel);
    std::fs::create_dir_all(&dir)?;
    let path = blob_path(channel, hash);
    if path.exists() {
        return Ok(()); // content-addressed: the same bytes are the same file
    }
    std::fs::write(&path, data)?;
    config::lock_down(&path);
    Ok(())
}

/// Reads a blob back, and refuses it if the bytes do not match the name it is
/// filed under. A store that hands back the wrong file quietly is worse than
/// one that has lost it.
pub fn read_blob(channel: &str, hash: &str) -> Option<Vec<u8>> {
    let data = std::fs::read(blob_path(channel, hash)).ok()?;
    (hash_bytes(&data) == hash).then_some(data)
}

pub fn forget_channel(channel: &str) {
    let _ = std::fs::remove_dir_all(store_dir(channel));
}

pub fn human(size: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut n = size as f64;
    let mut u = 0;
    while n >= 1024.0 && u < UNITS.len() - 1 {
        n /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{size} B")
    } else {
        format!("{n:.1} {}", UNITS[u])
    }
}
