//! The server owns the only complete copy. It is kept to yourself on disk —
//! encrypting it while the key sits in a file beside it would be theatre, but
//! leaving it world-readable is just carelessness.
use crate::config;
use crate::msg::Msg;
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

pub fn append(m: &Msg) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = config::history_path();
    let existed = path.exists();
    let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
    else {
        return;
    };
    if !existed {
        config::lock_down(&path);
    }
    if let Ok(line) = serde_json::to_string(m) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn read() -> Vec<Msg> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(f) = std::fs::File::open(config::history_path()) else {
        return Vec::new();
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Msg>(&l).ok())
        .collect()
}

/// Where sequence numbers must not restart from. Deleting a channel removes
/// messages, which would drop the highest number in the file — and a server
/// that then handed out a number it had used before would make a watcher's
/// "resume from #N" skip real messages. So the mark is kept separately.
pub fn seq_floor() -> i64 {
    std::fs::read_to_string(config::home(".collab-seq"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn set_seq_floor(n: i64) {
    let path = config::home(".collab-seq");
    let _ = std::fs::write(&path, n.to_string());
    config::lock_down(&path);
}

/// Removes a channel's messages. Returns how many went.
/// The highest sequence number a channel has reached, or 0 for one that has
/// never been spoken on.
pub fn head(channel: &str) -> i64 {
    read()
        .iter()
        .filter(|m| m.channel == channel)
        .map(|m| m.seq)
        .max()
        .unwrap_or(0)
}

pub fn purge(channel: &str) -> usize {
    let all = read();
    let highest = all.iter().map(|m| m.seq).max().unwrap_or(0);
    let keep: Vec<Msg> = all
        .iter()
        .filter(|m| m.channel != channel)
        .cloned()
        .collect();
    let removed = all.len() - keep.len();
    if removed == 0 {
        return 0;
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let text: String = keep
        .iter()
        .filter_map(|m| serde_json::to_string(m).ok())
        .map(|l| l + "\n")
        .collect();
    let path = config::history_path();
    if std::fs::write(&path, text).is_ok() {
        config::lock_down(&path);
    }
    drop(_guard);
    set_seq_floor(highest.max(seq_floor()));
    removed
}

pub fn filter(msgs: Vec<Msg>, channel: &str, since: i64) -> Vec<Msg> {
    msgs.into_iter()
        .filter(|m| m.seq > since && (channel.is_empty() || m.channel == channel))
        .collect()
}
