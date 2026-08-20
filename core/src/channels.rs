//! Channels, and the keys that open them.
//!
//! A channel is created by a person, in the app, and comes with 32 bytes of
//! real entropy. That is a deliberate change of shape: the old single key was
//! five words a person could copy by hand, which needed Argon2 to stretch it
//! into something not worth guessing. A key nobody types does not need
//! stretching — it can simply be the key.
//!
//! An AI cannot make one. That is not a security boundary on this machine,
//! where anything with a shell could write this file; it is a guardrail
//! against the failure that actually happens, which is an AI inventing a
//! reasonable-sounding channel that matches nothing on the other machine and
//! calling into a room with nobody in it. Across machines it is a real
//! boundary: without the key there is no way in.
use crate::config;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Channel {
    /// 32 random bytes, base64. Not a passphrase; nobody is meant to read it.
    pub key: String,
    #[serde(default)]
    pub created: String,
    /// Whether this machine made it, or was given it.
    #[serde(default)]
    pub mine: bool,
}

pub type Registry = BTreeMap<String, Channel>;

pub fn path() -> PathBuf {
    config::home(".collab-channels.json")
}

pub fn load() -> Registry {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(reg: &Registry) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(reg)?;
    std::fs::write(path(), text)?;
    config::lock_down(&path()); // the one file worth stealing
    Ok(())
}

pub fn names() -> Vec<String> {
    load().keys().cloned().collect()
}

pub fn get(name: &str) -> Option<Channel> {
    load().get(name).cloned()
}

pub fn key_bytes(name: &str) -> Option<Vec<u8>> {
    let c = get(name)?;
    let raw = B64.decode(c.key.trim()).ok()?;
    (raw.len() == KEY_BYTES).then_some(raw)
}

/// A clean name: what a person typed, reduced to something that can be matched
/// exactly on the other machine without argument about spacing or case.
pub fn tidy(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect()
}

pub fn create(name: &str) -> Result<(String, String), String> {
    let name = tidy(name);
    if name.is_empty() {
        return Err("a channel needs a name".into());
    }
    let mut reg = load();
    if reg.contains_key(&name) {
        return Err(format!("#{name} already exists on this machine"));
    }
    let key = B64.encode(crate::crypto::random(KEY_BYTES));
    reg.insert(
        name.clone(),
        Channel { key: key.clone(), created: crate::msg::now(), mine: true },
    );
    save(&reg).map_err(|e| e.to_string())?;
    Ok((name, key))
}

/// Adding a channel somebody else made, from the name and key they sent you.
pub fn add(name: &str, key: &str) -> Result<String, String> {
    let name = tidy(name);
    if name.is_empty() {
        return Err("a channel needs a name".into());
    }
    let key = key.trim().to_string();
    match B64.decode(&key) {
        Ok(raw) if raw.len() == KEY_BYTES => {}
        Ok(raw) => return Err(format!("that key is {} bytes, not {KEY_BYTES}", raw.len())),
        Err(_) => return Err("that key is not valid base64".into()),
    }
    let mut reg = load();
    reg.insert(name.clone(), Channel { key, created: crate::msg::now(), mine: false });
    save(&reg).map_err(|e| e.to_string())?;
    Ok(name)
}

pub fn forget(name: &str) -> Result<(), String> {
    let mut reg = load();
    if reg.remove(&tidy(name)).is_none() {
        return Err("no such channel here".into());
    }
    save(&reg).map_err(|e| e.to_string())
}
