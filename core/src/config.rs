//! Where settings come from, and who you are.
//!
//! The environment first, then ~/.collab-config, then the default. The file
//! matters more than it looks: a collab started by the MCP server, by launchd,
//! or by clicking a notification inherits none of your shell, so a setting that
//! lives only in .zshrc works in a terminal and quietly fails everywhere else —
//! under a different name, on the wrong channel, or with no key at all.
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

pub fn home(name: &str) -> PathBuf {
    let h = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(h).join(name)
}

pub fn config_path() -> PathBuf {
    home(".collab-config")
}
pub fn history_path() -> PathBuf {
    home(".collab-history.jsonl")
}
pub fn seen_path() -> PathBuf {
    home(".collab-seen")
}

fn values() -> &'static HashMap<String, String> {
    static VALUES: OnceLock<HashMap<String, String>> = OnceLock::new();
    VALUES.get_or_init(|| {
        let mut m = HashMap::new();
        let Ok(text) = std::fs::read_to_string(config_path()) else {
            return m;
        };
        for line in text.lines() {
            let line = match line.split_once('#') {
                Some((before, _)) => before,
                None => line,
            };
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim().to_lowercase(), v.trim());
                if !k.is_empty() && !v.is_empty() {
                    m.insert(k, v.to_string());
                }
            }
        }
        m
    })
}

/// env("COLLAB_NAME", "…") — checks $COLLAB_NAME, then `name =` in the config.
pub fn env(key: &str, default: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return v;
        }
    }
    let short = key.strip_prefix("COLLAB_").unwrap_or(key).to_lowercase();
    if let Some(v) = values().get(&short) {
        return v.clone();
    }
    default.to_string()
}

/// Where a setting came from — half of every confusing moment with this thing.
pub fn source(key: &str) -> String {
    if std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false) {
        return format!("from ${key}");
    }
    let short = key.strip_prefix("COLLAB_").unwrap_or(key).to_lowercase();
    if values().contains_key(&short) {
        return format!("from {}", config_path().display());
    }
    "default".into()
}

pub fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

pub fn name() -> String {
    env("COLLAB_NAME", &hostname())
}
/// The name to answer to. A chat that has named itself answers to that and not
/// to the machine it runs on — the whole reason chats have their own names is
/// that "@tankun" means the person, not whichever of their sessions happens to
/// be listening. Anything without a chat name is the person, and answers to the
/// machine name.
/// A display name is a person's to choose, but it still has to be one word the
/// mention parser can find, so it is tidied the same way a channel name is.
pub fn tidy_name(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(24)
        .collect()
}

/// Every name this chat answers to on a given channel. A chat that has named
/// itself answers to that; a person answers to whatever they chose on that
/// channel, and to the machine name either way.
pub fn my_names_on(channel: &str) -> Vec<String> {
    match session_name() {
        Some(n) => vec![n],
        None => crate::channels::names_on(channel),
    }
}

pub fn channel() -> String {
    env("COLLAB_CHANNEL", "general")
}
pub fn port() -> String {
    env("COLLAB_PORT", "8787")
}
pub fn addr() -> String {
    format!("{}:{}", env("COLLAB_HOST", "localhost"), port())
}
pub fn notify_enabled() -> bool {
    env("COLLAB_NOTIFY", "1") != "0"
}

/// chmod 600. Encrypting the history while the key sits in a world-readable
/// file beside it would be theatre; keeping both to yourself is not.
pub fn lock_down(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The same answers as `who`, for the app.
pub fn who_json() {
    let j = serde_json::json!({
        "name": name(),
        "channel": channel(),
        "addr": addr(),
        "channels": crate::channels::names(),
        "notifier": crate::notify::find_notifier().map(|p| p.display().to_string()),
    });
    println!("{j}");
}

/// Claude Code gives an MCP server and anything a Monitor runs the same
/// CLAUDE_CODE_SESSION_ID, which is what lets one chat's `collab watch` know
/// which messages that same chat sent. Empty anywhere else — a plain terminal,
/// or the app under launchd — and then nothing is suppressed, which is right.
pub fn session_id() -> String {
    std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default()
}

fn sessions_dir() -> PathBuf {
    home(".collab-sessions")
}

/// Who a chat is and where it is talking. Written by collab_join, read by that
/// same chat's watcher — which is how it recognises its own messages, and how
/// it knows to follow the channel the chat actually joined rather than the
/// machine's default.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
    #[serde(default)]
    pub name: String,
    /// Every channel this chat is listening to. A chat can work on the shop and
    /// the lobby at once, and hearing only one of them would be worse than
    /// hearing neither — it would look like the other had gone quiet.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Older files held a single channel here.
    #[serde(default, skip_serializing)]
    pub channel: String,
}

impl Session {
    pub fn listening(&self) -> Vec<String> {
        if !self.channels.is_empty() {
            self.channels.clone()
        } else if !self.channel.is_empty() {
            vec![self.channel.clone()]
        } else {
            Vec::new()
        }
    }
}

pub fn session() -> Option<Session> {
    let id = session_id();
    if id.is_empty() {
        return None;
    }
    let raw = std::fs::read_to_string(sessions_dir().join(id)).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Files written before channels were part of this hold a bare name.
    Some(serde_json::from_str(raw).unwrap_or(Session {
        name: raw.to_string(),
        ..Default::default()
    }))
}

pub fn session_name() -> Option<String> {
    session().map(|s| s.name).filter(|s| !s.is_empty())
}

/// Every channel this chat is listening to. Empty means it has not subscribed.
pub fn session_channels() -> Vec<String> {
    session().map(|s| s.listening()).unwrap_or_default()
}

pub fn save_session(name: &str, chans: &[String]) {
    let id = session_id();
    if id.is_empty() {
        return;
    }
    let dir = sessions_dir();
    let _ = std::fs::create_dir_all(&dir);
    let s = Session {
        name: name.into(),
        channels: chans.to_vec(),
        channel: String::new(),
    };
    if let Ok(text) = serde_json::to_string(&s) {
        let final_path = dir.join(&id);
        let tmp = dir.join(format!(".{id}.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            if std::fs::rename(&tmp, &final_path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            } else {
                lock_down(&final_path);
            }
        }
    }
    prune_sessions(&dir);
}

/// Chats end without saying so, so their files would pile up for ever.
fn prune_sessions(dir: &std::path::Path) {
    let week = std::time::Duration::from_secs(7 * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|age| age > week).unwrap_or(false))
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

pub fn who() {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "name     {:<28} {}", name(), source("COLLAB_NAME"));
    let _ = writeln!(
        out,
        "channel  {:<28} {}",
        channel(),
        source("COLLAB_CHANNEL")
    );
    let _ = writeln!(out, "server   {:<28} {}", addr(), source("COLLAB_HOST"));
    let chans = crate::channels::names();
    let list = if chans.is_empty() {
        "none — make one in the collab app".to_string()
    } else {
        chans.join(", ")
    };
    let _ = writeln!(out, "channels {list}");
    match crate::notify::find_notifier() {
        Some(h) => {
            let state = if notify_enabled() {
                "on"
            } else {
                "off (COLLAB_NOTIFY=0)"
            };
            let _ = writeln!(out, "popups   {:<28} {}", state, h.display());
        }
        None => {
            let _ = writeln!(out, "popups   {:<28} no notifier installed", "unavailable");
        }
    }
}
