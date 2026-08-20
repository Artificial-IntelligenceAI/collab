//! watch, post, change, log — and the rule that a dropped message must never
//! look like silence.
use crate::channels;
use crate::config;
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

fn dial(channel: &str) -> std::io::Result<Conn> {
    let stream = TcpStream::connect(config::addr())?;
    let _ = stream.set_nodelay(true);
    Conn::connect(stream, channel)
}

/// Dials one channel, delivers, and on failure reconnects — announcing both,
/// because silence must always mean "nobody is talking" and never "the wire
/// died". Returns when `want` names a different channel from the one it is on:
/// a chat can join a channel after its watcher has already started, and staying
/// on the old one would be that same silence by another route.
fn stream_channel<F, S>(
    channel: &str,
    since: &std::sync::atomic::AtomicI64,
    want: &dyn Fn() -> String,
    on_msg: &mut F,
    on_status: &mut S,
) where
    F: FnMut(&Msg),
    S: FnMut(bool, i64, Option<String>),
{
    use std::sync::atomic::Ordering::SeqCst;
    let mut announced = false;
    loop {
        if want() != channel {
            return;
        }
        match dial(channel) {
            Err(e) => {
                if !announced {
                    on_status(false, since.load(SeqCst), Some(e.to_string()));
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Ok(mut conn) => {
                announced = false;
                on_status(true, since.load(SeqCst), None);
                let hello = Hello {
                    name: config::name(),
                    host: config::name(),
                    channel: channel.to_string(),
                    since: since.load(SeqCst),
                    mode: "watch".into(),
                };
                if let Err(e) = conn.send(&hello).and_then(|_| {
                    conn.expect_welcome()
                        .map(|w| channels::learn_creator(channel, &w.creator))
                }) {
                    on_status(false, since.load(SeqCst), Some(e.to_string()));
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                // A short read timeout is what lets a joined-channel change be
                // noticed while the channel is quiet, without a poll loop.
                conn.set_read_timeout(Some(Duration::from_secs(2)));
                loop {
                    match conn.recv::<Msg>() {
                        Ok(Some(m)) => on_msg(&m),
                        Ok(None) => break,
                        Err(e) if is_timeout(&e) => {
                            if want() != channel {
                                return;
                            }
                        }
                        Err(e) => {
                            on_status(false, since.load(SeqCst), Some(e.to_string()));
                            announced = true;
                            break;
                        }
                    }
                }
                if !announced {
                    on_status(false, since.load(SeqCst), None);
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

pub fn watch(as_json: bool, popups: bool, since: Option<i64>, save: bool, all: bool) -> ! {
    if all {
        // One connection per channel: a connection belongs to a channel now,
        // because it is that channel's key that opens it. Each keeps its own
        // place, since one seen-file cannot stand for several channels.
        let mut handles = Vec::new();
        for name in channels::names() {
            let start = since.unwrap_or(0);
            handles.push(std::thread::spawn(move || {
                watch_one(&name, start, false, as_json, popups, &|| name.clone())
            }));
        }
        if handles.is_empty() {
            report_no_channels(as_json);
        }
        for h in handles {
            let _ = h.join();
        }
        std::process::exit(0)
    }

    // A chat follows the channel it joined; anything else follows the config.
    let want = || {
        if config::session_id().is_empty() {
            config::channel()
        } else {
            config::session_channel()
        }
    };
    loop {
        let here = want();
        if channels::key_bytes(&here).is_none() {
            report_unknown(&here, as_json);
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        watch_one(&here, since.unwrap_or_else(last_seen), save, as_json, popups, &want);
    }
}

fn report_no_channels(as_json: bool) -> ! {
    if as_json {
        println!("{}", serde_json::json!({"type":"status","connected":false,
            "error":"no channels on this machine yet","addr":config::addr(),"from":0}));
    } else {
        eprintln!("collab: no channels on this machine yet — make one in the collab app");
    }
    let _ = std::io::stdout().flush();
    std::thread::sleep(Duration::from_secs(3600));
    std::process::exit(0)
}

fn report_unknown(name: &str, as_json: bool) {
    if as_json {
        println!("{}", serde_json::json!({"type":"status","connected":false,
            "error":format!("no key for #{name} on this machine"),
            "addr":config::addr(),"from":0}));
        let _ = std::io::stdout().flush();
    }
}

fn watch_one(
    channel: &str,
    start: i64,
    save: bool,
    as_json: bool,
    popups: bool,
    want: &dyn Fn() -> String,
) {
    let mut warned = false;
    let mut first = true;
    let notifier = if popups { Notifier::new(config::name()) } else { None };
    let addr = config::addr();
    let host = config::name();
    let cursor = std::sync::atomic::AtomicI64::new(start);

    let mut on_msg = |m: &Msg| {
        use std::sync::atomic::Ordering::SeqCst;
        // A chat does not need its own words read back to it. Only this chat's
        // own are dropped — a sibling chat on the same machine is someone else,
        // and worth hearing. The place in the sequence still advances, so a
        // resume after this is still exact.
        let mine = m.via == crate::msg::ACTOR_AI
            && m.host == host
            && config::session_name().is_some_and(|n| n == m.from);
        cursor.store(m.seq, SeqCst);
        if save {
            save_seen(m.seq, &mut warned);
        }
        if mine {
            return;
        }
        if as_json {
            if let Ok(s) = serde_json::to_string(&serde_json::json!({"type":"msg","msg":m})) {
                println!("{s}");
            }
        } else {
            println!("[{}] {}: {}", m.channel, m.label(), m.line());
        }
        let _ = std::io::stdout().flush();
        if let Some(n) = &notifier {
            n.send(m);
        }
    };

    let mut on_status = |up: bool, from: i64, err: Option<String>| {
        if as_json {
            println!("{}", serde_json::json!({
                "type":"status","connected":up,"from":from,"addr":addr,
                "channel":channel,"error":err}));
            let _ = std::io::stdout().flush();
            return;
        }
        if up {
            if !first {
                println!("* reconnected to {addr} #{channel}, resuming from #{from}");
            }
            first = false;
        } else {
            first = false;
            match err {
                Some(e) => println!("* DISCONNECTED from {addr} #{channel} — {e} — retrying"),
                None => println!("* DISCONNECTED from {addr} #{channel} — retrying"),
            }
        }
        let _ = std::io::stdout().flush();
    };

    stream_channel(channel, &cursor, want, &mut on_msg, &mut on_status);
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
    let mut conn = dial(channel)?;
    conn.send(&Hello {
        name,
        host,
        channel: channel.to_string(),
        mode: "post".into(),
        since: 0,
    })?;
    // Refuses to call an unreadable message "sent", and learns who made the
    // channel while it is here.
    channels::learn_creator(channel, &conn.expect_welcome()?.creator);
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
    // No channel named means every channel this machine holds a key for —
    // which is now several connections, because a connection is a channel.
    if channel.is_empty() {
        let mut all: Vec<Msg> = channels::names()
            .iter()
            .flat_map(|c| fetch(c, since))
            .collect();
        all.sort_by_key(|m| m.seq);
        all.dedup_by_key(|m| m.seq);
        return all;
    }
    let local = || history::filter(history::read(), channel, since);
    let Ok(mut conn) = dial(channel) else { return local() };
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

/// Channels a person can see and join. Creating one is deliberately not here:
/// it happens in the app, by a person, because a key made on a whim by
/// something that cannot copy it to the other machine is a room with nobody
/// in it.
pub fn channels_cmd(show_keys: bool, as_json: bool) {
    let reg = channels::load();
    if as_json {
        let list: Vec<_> = reg
            .iter()
            .map(|(name, ch)| {
                serde_json::json!({"name": name, "key": ch.key, "mine": ch.mine,
                                   "created": ch.created, "creator": ch.creator_name()})
            })
            .collect();
        println!("{}", serde_json::json!(list));
        return;
    }
    if reg.is_empty() {
        println!("no channels on this machine yet — make one in the collab app");
        return;
    }
    for (name, ch) in reg {
        let origin = if ch.mine { "made here" } else { "joined" };
        if show_keys {
            println!("#{name}  ({origin})\n    key = {}", ch.key);
        } else {
            println!("#{name}  ({origin})");
        }
    }
    if !show_keys {
        println!("\n(collab channels -keys shows the keys, to copy to the other machine)");
    }
}

/// What the app's button calls. There is deliberately no MCP tool for this:
/// the CLI is a person's surface, the same as the button is.
pub fn channel_create(name: &str) {
    match channels::create(name) {
        Ok((n, key)) => {
            println!("#{n}");
            println!("{key}");
        }
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(2);
        }
    }
}

pub fn channel_add(name: &str, key: &str) {
    match channels::add(name, key, "") {
        Ok(n) => println!("joined #{n} — it will work once the other machine is reachable"),
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(2);
        }
    }
}

/// Closing a channel everywhere, as opposed to leaving it. The local check is
/// only for a decent error message; the server does its own, because a machine
/// that has been told it may not delete could simply not ask.
pub fn channel_delete(name: &str) {
    let name = channels::tidy(name);
    if let Err(e) = channels::may_delete(&name) {
        eprintln!("collab: {e}");
        std::process::exit(2);
    }
    let mut conn = match dial(&name) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("collab: cannot reach {} — {e}", config::addr());
            eprintln!("        the server has to be reachable to delete a channel from it");
            std::process::exit(1);
        }
    };
    let hello = Hello {
        name: config::name(),
        host: config::name(),
        channel: name.clone(),
        since: 0,
        mode: "delete".into(),
    };
    if let Err(e) = conn.send(&hello).and_then(|_| conn.expect_welcome().map(|_| ())) {
        eprintln!("collab: {e}");
        std::process::exit(1);
    }
    match conn.recv::<crate::wire::Ack>() {
        Ok(Some(a)) if a.ok => {
            let _ = channels::forget(&name);
            println!("{}", a.detail);
            println!("the key is gone from here; anyone else still holding it can no longer connect");
        }
        Ok(Some(a)) => {
            eprintln!("collab: {}", a.detail);
            std::process::exit(2);
        }
        _ => {
            eprintln!("collab: the server did not answer");
            std::process::exit(1);
        }
    }
}

pub fn channel_forget(name: &str) {
    match channels::forget(name) {
        Ok(()) => println!("forgot #{}", channels::tidy(name)),
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(2);
        }
    }
}
