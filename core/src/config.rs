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

/// The shared word. Both machines need the same one; without it there is no
/// conversation at all, which is the failure you want.
pub fn key() -> Option<String> {
    let k = env("COLLAB_KEY", "");
    if k.is_empty() {
        None
    } else {
        Some(k)
    }
}

/// Writes `key = …` into the config, creating it, and locks the file down —
/// it is the one file whose contents are worth stealing.
pub fn save_key(k: &str) -> std::io::Result<()> {
    let path = config_path();
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if values().contains_key("key") {
        text = text
            .lines()
            .filter(|l| {
                !l.split_once('=')
                    .map(|(a, _)| a.trim().eq_ignore_ascii_case("key"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("key = {k}\n"));
    std::fs::write(&path, text)?;
    lock_down(&path);
    Ok(())
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
        "hasKey": key().is_some(),
        "notifier": crate::notify::find_notifier().map(|p| p.display().to_string()),
    });
    println!("{j}");
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
    match key() {
        Some(_) => {
            let _ = writeln!(
                out,
                "key      {:<28} {}",
                "set (messages encrypted)",
                source("COLLAB_KEY")
            );
        }
        None => {
            let _ = writeln!(out, "key      {:<28} run: collab key -new", "MISSING");
        }
    }
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
            let _ = writeln!(
                out,
                "popups   {:<28} no notifier installed",
                "unavailable"
            );
        }
    }
}
