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
    /// The machine that made it. `mine` is only this machine's opinion; the
    /// server needs a name it can check against whoever is asking to delete.
    #[serde(default)]
    pub creator: String,
    /// What to call yourself in this channel. A person is one name to their
    /// family and another to a work project, and the machine's name is nobody's
    /// choice at all — it is whatever the computer was called in a shop. Empty
    /// means fall back to the machine name.
    #[serde(default)]
    pub display: String,
}

impl Channel {
    /// Entries written before creators were recorded: if this machine made it,
    /// this machine is the creator.
    pub fn creator_name(&self) -> String {
        if self.creator.is_empty() && self.mine {
            config::name()
        } else {
            self.creator.clone()
        }
    }
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
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'))
        .take(32)
        .collect()
}

/// An invite is the channel's name and its key in one string, so joining is
/// paste-one-thing and both machines end up calling the room the same name —
/// the way joining a group chat works. `:` is the separator because a channel
/// name cannot contain one (tidy strips it) and base64 never produces one.
/// What this machine calls itself on one channel, or nothing if it has not been
/// asked yet.
pub fn display_for(channel: &str) -> Option<String> {
    load()
        .get(&tidy(channel))
        .map(|c| c.display.trim().to_string())
        .filter(|d| !d.is_empty())
}

/// Sets it. An empty string clears it, which puts the machine name back.
pub fn set_display(channel: &str, display: &str) -> Result<(), String> {
    let ch = tidy(channel);
    let mut reg = load();
    let Some(entry) = reg.get_mut(&ch) else {
        return Err(format!("no channel #{ch} on this machine"));
    };
    entry.display = crate::config::tidy_name(display);
    save(&reg).map_err(|e| e.to_string())
}

/// Every name this machine answers to on a channel: the one chosen for it, and
/// the machine name, which stays valid because it is what the other side sees
/// before a choice is made.
pub fn names_on(channel: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(d) = display_for(channel) {
        out.push(d);
    }
    let machine = crate::config::name();
    if !out.iter().any(|n| n.eq_ignore_ascii_case(&machine)) {
        out.push(machine);
    }
    out
}

pub fn invite(name: &str, key: &str) -> String {
    format!("{name}:{key}")
}

/// Splits an invite back apart. A bare key is still accepted everywhere an
/// invite is, so anything already shared keeps working — it just cannot carry
/// a name, and the caller has to supply one.
pub fn split_invite(s: &str) -> (Option<String>, String) {
    let s = s.trim();
    match s.split_once(':') {
        Some((n, k)) if !n.is_empty() && !k.is_empty() => (Some(tidy(n)), k.trim().to_string()),
        _ => (None, s.to_string()),
    }
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
        Channel {
            key: key.clone(),
            created: crate::msg::now(),
            mine: true,
            creator: config::name(),
            display: String::new(),
        },
    );
    save(&reg).map_err(|e| e.to_string())?;
    Ok((name, key))
}

/// A new key for a channel made here. Nothing already said is lost — history is
/// kept decrypted on this machine — but every other machine holding the old key
/// stops being able to open a frame, which is the whole point: rotation is how a
/// leaked key stops mattering. Nobody is thrown out of a conversation so much as
/// required to be re-invited to it, so this hands back the new invite to send.
///
/// The running server picks it up without a restart: it re-reads the registry on
/// every connection rather than holding keys in memory.
///
/// Only for channels made here, for the same reason `may_delete` is. Rotating a
/// channel somebody else made would not secure it — it would remove them from it
/// using the key they gave you in good faith.
pub fn rotate(name: &str) -> Result<(String, String), String> {
    let name = tidy(name);
    let mut reg = load();
    let ch = reg
        .get_mut(&name)
        .ok_or_else(|| format!("no channel #{name} here"))?;
    if !ch.mine {
        let who = if ch.creator_name().is_empty() {
            "whoever made it".to_string()
        } else {
            ch.creator_name()
        };
        return Err(format!(
            "#{name} was joined, not made here — a new key would lock out {who}, \
             using the key they gave you. Ask {who} to rotate it instead"
        ));
    }
    let key = B64.encode(crate::crypto::random(KEY_BYTES));
    ch.key = key.clone();
    save(&reg).map_err(|e| e.to_string())?;
    Ok((name, key))
}

/// Adding a channel somebody else made, from the name and key they sent you.
pub fn add(name: &str, key: &str, creator: &str) -> Result<String, String> {
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
    // A channel is identified by its key and named locally, so the same key
    // added twice makes two names for one room. The server finds a channel by
    // trial-decrypting, takes whichever name it meets first, and messages then
    // arrive on a channel nobody addressed — a post to #window landing on
    // #tankun is how this was found. One name per key.
    if let Some((held, _)) = reg.iter().find(|(n, c)| c.key == key && **n != name) {
        return Err(format!(
            "this machine already holds that key as #{held}. A channel is its key — adding it again under another name makes two names for one room, and messages then turn up on whichever name was found first.\n\n  to rename it: collab channel forget {held}, then add it again as #{name}"
        ));
    }
    // The mirror of the check above, and the one that was missing: the same
    // *name* added twice with different keys. That silently replaced the key,
    // leaving two machines each certain they were on #window and neither able
    // to open the other's frames. It ran for two days without a symptom,
    // because a channel nobody posts to looks exactly like a channel that
    // works — the only thing that ever reported it was a status light, which
    // blamed a different channel.
    //
    // Adding the key you already hold under the name you already hold is not
    // an error; it is somebody pasting the same invite twice.
    if let Some(held) = reg.get(&name) {
        if held.key != key {
            let origin = if held.mine { "made here" } else { "joined" };
            return Err(format!(
                "#{name} already exists on this machine ({origin}) with a different key, and \
                 adding this one would replace it silently. Two machines would then each hold \
                 a #{name} the other cannot open, which looks like nothing at all until \
                 somebody wonders why a room is quiet.\n\n  \
                 to replace it deliberately: collab channel forget {name}, then add it again\n  \
                 to keep both: add this one under a different name"
            ));
        }
    }
    reg.insert(
        name.clone(),
        Channel {
            key,
            created: crate::msg::now(),
            mine: false,
            creator: creator.trim().to_string(),
            display: String::new(),
        },
    );
    save(&reg).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Records who made a channel, learnt from the server on connecting. Someone
/// who was handed a key has no other way to know.
pub fn learn_creator(name: &str, creator: &str) {
    let creator = creator.trim();
    if creator.is_empty() {
        return;
    }
    let mut reg = load();
    if let Some(ch) = reg.get_mut(name) {
        if ch.creator.is_empty() && !ch.mine {
            ch.creator = creator.to_string();
            let _ = save(&reg);
        }
    }
}

/// Drops the key from this machine only. Everyone else's copy is untouched —
/// leaving a room is not the same as closing it.
pub fn forget(name: &str) -> Result<(), String> {
    let mut reg = load();
    if reg.remove(&tidy(name)).is_none() {
        return Err("no such channel here".into());
    }
    save(&reg).map_err(|e| e.to_string())
}

/// Closing the room. Only the machine that made the channel may, and the
/// server checks that itself rather than taking the asker's word for it.
pub fn may_delete(name: &str) -> Result<Channel, String> {
    let name = tidy(name);
    let ch = get(&name).ok_or_else(|| format!("no channel #{name} here"))?;
    let creator = ch.creator_name();
    if creator.is_empty() {
        return Err(format!(
            "#{name} does not record which machine made it, so it cannot be deleted — \
             you can leave it instead"
        ));
    }
    if creator != config::name() {
        return Err(format!(
            "#{name} was made on {creator}, and only {creator} can delete it. \
             You can leave it instead, which drops your key without touching anyone else's"
        ));
    }
    Ok(ch)
}
