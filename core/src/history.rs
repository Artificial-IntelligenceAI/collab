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

pub fn filter(msgs: Vec<Msg>, channel: &str, since: i64) -> Vec<Msg> {
    msgs.into_iter()
        .filter(|m| m.seq > since && (channel.is_empty() || m.channel == channel))
        .collect()
}
