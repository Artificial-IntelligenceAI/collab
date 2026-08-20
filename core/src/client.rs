//! watch, post, change, log — and the rule that a dropped message must never
//! look like silence.
use crate::config;
use crate::crypto;
use crate::history;
use crate::msg::{Msg, ACTIONS, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use crate::notify::Notifier;
use crate::wire::{Conn, Hello};
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

pub fn last_seen() -> i64 {
    std::fs::read_to_string(config::seen_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Losing our place is not allowed to be quiet either: if we cannot write it
/// down we would replay the whole history on the next reconnect and never say why.
pub fn save_seen(n: i64, warned: &mut bool) {
    if let Err(e) = std::fs::write(config::seen_path(), n.to_string()) {
        if !*warned {
            *warned = true;
            eprintln!(
                "* cannot record my place in {} ({e}) — after a reconnect you may see old messages again",
                config::seen_path().display()
            );
        }
    }
}

fn dial() -> std::io::Result<Conn> {
    let stream = TcpStream::connect(config::addr())?;
    let _ = stream.set_nodelay(true);
    Conn::connect(stream)
}

/// Dials, delivers, and on failure reconnects — announcing both, because
/// silence must always mean "nobody is talking" and never "the wire died".
pub fn stream<F, S>(channel: &str, since: impl Fn() -> i64, mut on_msg: F, mut on_status: S) -> !
where
    F: FnMut(&Msg),
    S: FnMut(bool, i64, Option<String>),
{
    let mut announced = false;
    loop {
        match dial() {
            Err(e) => {
                if !announced {
                    on_status(false, since(), Some(e.to_string()));
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            Ok(mut conn) => {
                announced = false;
                on_status(true, since(), None);
                let hello = Hello {
                    name: config::name(),
                    host: config::name(),
                    channel: channel.to_string(),
                    since: since(),
                    mode: "watch".into(),
                };
                if let Err(e) = conn
                    .send(&hello)
                    .and_then(|_| conn.expect_welcome().map(|_| ()))
                {
                    on_status(false, since(), Some(e.to_string()));
                    announced = true;
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                {
                    loop {
                        match conn.recv::<Msg>() {
                            Ok(Some(m)) => on_msg(&m),
                            Ok(None) => break,
                            Err(e) => {
                                // A frame that will not open is not a dropped
                                // connection — say which it was.
                                on_status(false, since(), Some(e.to_string()));
                                announced = true;
                                break;
                            }
                        }
                    }
                }
                if !announced {
                    on_status(false, since(), None);
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

/// `since`: where to start, instead of ~/.collab-seen.
/// `save`: whether to write our place back to ~/.collab-seen. The app passes
/// false — two watchers sharing that one file would overwrite each other's
/// place, and the Monitor's watcher is the one that owns it.
/// `all`: every channel rather than only this one; the views do the filtering.
pub fn watch(as_json: bool, popups: bool, since: Option<i64>, save: bool, all: bool) -> ! {
    let mut warned = false;
    let mut first = true;
    let notifier = if popups {
        Notifier::new(config::name())
    } else {
        None
    };
    let channel = if all { String::new() } else { config::channel() };
    let addr = config::addr();

    // Our place in the sequence, held here rather than re-read from the file,
    // so a reconnect resumes from what we actually have and not from zero.
    let cursor = std::sync::atomic::AtomicI64::new(since.unwrap_or_else(last_seen));
    let read_cursor = || cursor.load(std::sync::atomic::Ordering::SeqCst);

    stream(
        &channel,
        read_cursor,
        |m| {
            if as_json {
                if let Ok(s) = serde_json::to_string(&serde_json::json!({"type":"msg","msg":m})) {
                    println!("{s}");
                }
            } else {
                println!("[{}] {}: {}", m.channel, m.label(), m.line());
            }
            let _ = std::io::stdout().flush();
            cursor.store(m.seq, std::sync::atomic::Ordering::SeqCst);
            if save {
                save_seen(m.seq, &mut warned);
            }
            if let Some(n) = &notifier {
                n.send(m);
            }
        },
        |up, from, err| {
            if as_json {
                let ev = serde_json::json!({
                    "type": "status", "connected": up, "from": from,
                    "addr": addr, "error": err,
                });
                println!("{ev}");
                let _ = std::io::stdout().flush();
                return;
            }
            if up {
                if !first {
                    println!("* reconnected to {addr}, resuming from #{from}");
                }
                first = false;
            } else {
                first = false;
                match err {
                    Some(e) => println!("* DISCONNECTED from {addr} — {e} — retrying"),
                    None => println!("* DISCONNECTED from {addr} — retrying"),
                }
            }
            let _ = std::io::stdout().flush();
        },
    )
}

/// Delivers one message on $COLLAB_CHANNEL, under this machine's own name.
pub fn send(m: Msg) -> std::io::Result<()> {
    send_full(&config::channel(), None, m)
}

/// The same, under a display name of your own. An AI session names itself, so
/// two chats on one machine are two voices rather than one — but the machine
/// goes along regardless, because "which of us was that" has to stay answerable
/// even when the chosen name says nothing about it.
pub fn send_full(channel: &str, display: Option<&str>, m: Msg) -> std::io::Result<()> {
    let host = config::name();
    let name = display
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| host.clone());
    let mut conn = dial()?;
    conn.send(&Hello {
        name,
        host,
        channel: channel.to_string(),
        mode: "post".into(),
        since: 0,
    })?;
    conn.expect_welcome()?; // refuses to call an unreadable message "sent"
    conn.send(&m)?;
    std::thread::sleep(Duration::from_millis(150)); // let it land before hanging up
    Ok(())
}

fn fail(e: std::io::Error) -> ! {
    eprintln!("collab: cannot reach {} — {e}", config::addr());
    std::process::exit(1)
}

pub fn post(text: &str, via_ai: bool) {
    let text = text.trim().replace('\n', " ");
    if text.is_empty() {
        eprintln!("usage: collab post \"message\"");
        std::process::exit(2);
    }
    let m = Msg {
        kind: KIND_CHAT.into(),
        via: if via_ai {
            ACTOR_AI.into()
        } else {
            String::new()
        },
        text,
        ..Default::default()
    };
    if let Err(e) = send(m) {
        fail(e)
    }
}

pub const CHANGE_USAGE: &str = "usage: collab change -action added|edited|removed|renamed -target \"where\" \"one-line summary\"

  -action   what you did to it
  -target   which script or instance, e.g. ServerScriptService/ShopHandler
  summary   one line, in past tense: \"gave the buy button a debounce\"";

pub fn change(action: &str, target: &str, summary: &str, via_ai: bool) {
    let action = action.trim().to_lowercase();
    if !ACTIONS.contains(&action.as_str()) {
        eprintln!(
            "collab: -action must be one of {}\n\n{CHANGE_USAGE}",
            ACTIONS.join(", ")
        );
        std::process::exit(2);
    }
    let summary = summary.trim().replace('\n', " ");
    if target.trim().is_empty() || summary.is_empty() {
        eprintln!("{CHANGE_USAGE}");
        std::process::exit(2);
    }
    let m = Msg {
        kind: KIND_CHANGE.into(),
        via: if via_ai {
            ACTOR_AI.into()
        } else {
            String::new()
        },
        action,
        target: target.trim().into(),
        text: summary,
        ..Default::default()
    };
    if let Err(e) = send(m) {
        fail(e)
    }
}

/// The server owns the only complete history, so ask over the wire; fall back
/// to whatever is local rather than claiming the channel is empty.
pub fn fetch(channel: &str, since: i64) -> Vec<Msg> {
    let local = || history::filter(history::read(), channel, since);
    let Ok(mut conn) = dial() else { return local() };
    let hello = Hello {
        name: config::name(),
        host: config::name(),
        channel: channel.to_string(),
        since,
        mode: "fetch".into(),
    };
    if let Err(e) = conn
        .send(&hello)
        .and_then(|_| conn.expect_welcome().map(|_| ()))
    {
        // Falling back silently would make a key mismatch look like a quiet channel.
        eprintln!("collab: {e}");
        return local();
    }
    let mut out = Vec::new();
    while let Ok(Some(m)) = conn.recv::<Msg>() {
        out.push(m);
    }
    if out.is_empty() {
        return local();
    }
    out
}

pub fn show_log(only_changes: bool, all_channels: bool) {
    let channel = if all_channels {
        String::new()
    } else {
        config::channel()
    };
    for m in fetch(&channel, 0) {
        if only_changes && !m.is_change() {
            continue;
        }
        println!("#{:<4} [{}] {}: {}", m.seq, m.hhmm(), m.label(), m.line());
    }
}

pub fn key_cmd(make_new: bool) {
    if make_new {
        let k = crypto::new_key();
        if let Err(e) = config::save_key(&k) {
            eprintln!(
                "collab: cannot write {} — {e}",
                config::config_path().display()
            );
            std::process::exit(1);
        }
        println!("a new shared key is set on this machine:\n");
        println!("    key = {k}\n");
        println!("copy that line into ~/.collab-config on the other machine.");
        println!("until both sides match, they cannot talk to each other at all.");
        println!("\nthe server must be restarted to pick it up.");
        return;
    }
    match config::key() {
        Some(k) => {
            println!("key = {k}");
            println!(
                "\n(this is the line the other machine needs. `collab key -new` replaces it.)"
            );
        }
        None => {
            eprintln!("collab: no shared key set — run `collab key -new`");
            std::process::exit(1);
        }
    }
}
