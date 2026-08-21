//! watch, post, change, log — and the rule that a dropped message must never
//! look like silence.
use crate::channels;
use crate::config;
use crate::files;
use crate::history;
use crate::msg::{Msg, ACTIONS, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use crate::notify::Notifier;
use crate::wire::{Conn, Hello};
use std::net::TcpStream;
use std::time::Duration;

/// Where we got to, per channel. One number cannot stand for several channels:
/// a chat listening to the shop and the lobby is at a different point in each,
/// and a single mark would make one of them skip.
fn seen_map() -> std::collections::BTreeMap<String, i64> {
    std::fs::read_to_string(config::home(".collab-seen.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn seen_for(channel: &str) -> i64 {
    if let Some(n) = seen_map().get(channel) {
        return *n;
    }
    // Before channels each had their own, there was one file for the one channel.
    if channel == config::channel() {
        return std::fs::read_to_string(config::seen_path())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
    }
    0
}

/// Losing our place is not allowed to be quiet either: if we cannot write it
/// down we would replay a channel's whole history on the next reconnect and
/// never say why.
pub fn save_seen_for(channel: &str, n: i64, warned: &mut bool) {
    let mut map = seen_map();
    map.insert(channel.to_string(), n);
    let path = config::home(".collab-seen.json");
    let ok = serde_json::to_string(&map)
        .ok()
        .and_then(|t| std::fs::write(&path, t).ok())
        .is_some();
    if ok {
        config::lock_down(&path);
    } else if !*warned {
        *warned = true;
        eprintln!(
            "* cannot record my place in {} — after a reconnect you may see old messages again",
            path.display()
        );
    }
}

/// Writes a line, and leaves quietly if whoever was reading has gone.
///
/// `println!` panics when stdout is a pipe nobody holds open any more. In a
/// watcher that panic message goes to the same dead pipe, so it is printed
/// nowhere at all — and panicking while reporting a panic aborts. That is how
/// closing the window turned into a crash report, twenty-five times, with
/// nothing written in any log to say why. A reader leaving is not a failure.
/// It is the end of the job.
fn emit(line: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
        std::process::exit(0);
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
    still_wanted: &dyn Fn() -> bool,
    on_msg: &mut F,
    on_status: &mut S,
) where
    F: FnMut(&Msg, bool),
    S: FnMut(bool, i64, i64, Option<String>),
{
    use std::sync::atomic::Ordering::SeqCst;
    let mut announced = false;
    loop {
        if !still_wanted() {
            return;
        }
        match dial(channel) {
            Err(e) => {
                if !announced {
                    on_status(false, since.load(SeqCst), 0, Some(e.to_string()));
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Ok(mut conn) => {
                announced = false;
                let hello = Hello {
                    name: config::name(),
                    host: config::name(),
                    channel: channel.to_string(),
                    since: since.load(SeqCst),
                    mode: "watch".into(),
                };
                let welcome = match conn.send(&hello).and_then(|_| conn.expect_welcome()) {
                    Ok(w) => w,
                    Err(e) => {
                        on_status(false, since.load(SeqCst), 0, Some(e.to_string()));
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                channels::learn_creator(channel, &welcome.creator);
                // Where the channel stood before we arrived. A server too old
                // to say sends 0, which marks nothing as backlog — the same
                // behaviour as before, rather than a wrong claim about age.
                let head = welcome.head;
                on_status(true, since.load(SeqCst), head, None);
                // A short read timeout is what lets a joined-channel change be
                // noticed while the channel is quiet, without a poll loop.
                conn.set_read_timeout(Some(Duration::from_secs(2)));
                loop {
                    match conn.recv::<Msg>() {
                        Ok(Some(m)) => {
                            let replayed = head > 0 && m.seq <= head;
                            on_msg(&m, replayed);
                        }
                        Ok(None) => break,
                        Err(e) if is_timeout(&e) => {
                            if !still_wanted() {
                                return;
                            }
                        }
                        Err(e) => {
                            on_status(false, since.load(SeqCst), 0, Some(e.to_string()));
                            announced = true;
                            break;
                        }
                    }
                }
                if !announced {
                    on_status(false, since.load(SeqCst), 0, None);
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

/// Which channels this process should be listening to, right now. A chat's
/// subscriptions can change while it runs, so this is asked again rather than
/// decided once.
fn desired(all: bool) -> Vec<String> {
    if all {
        return channels::names();
    }
    if config::session_id().is_empty() {
        return vec![config::channel()];
    }
    config::session_channels()
}

pub fn watch(as_json: bool, popups: bool, since: Option<i64>, save: bool, all: bool) -> ! {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    // One connection per channel, because a connection is opened by a channel's
    // key. Threads are started as subscriptions appear and end by themselves
    // when the subscription goes, so the set can change without a restart.
    let running: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut complained: HashSet<String> = HashSet::new();
    let mut said_empty = false;

    loop {
        let want = desired(all);
        if want.is_empty() {
            if !said_empty {
                report_nothing_subscribed(as_json, all);
                said_empty = true;
            }
        } else {
            said_empty = false;
        }

        for ch in want {
            if channels::key_bytes(&ch).is_none() {
                if complained.insert(ch.clone()) {
                    report_unknown(&ch, as_json);
                }
                continue;
            }
            complained.remove(&ch);
            {
                let mut r = running.lock().unwrap();
                if r.contains(&ch) {
                    continue;
                }
                r.insert(ch.clone());
            }
            let running = Arc::clone(&running);
            let name = ch.clone();
            let start = since.unwrap_or_else(|| seen_for(&ch));
            std::thread::spawn(move || {
                let mine = name.clone();
                let still = move || desired(all).contains(&mine);
                watch_one(&name, start, save, as_json, popups, all, &still);
                running.lock().unwrap().remove(&name);
            });
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn report_nothing_subscribed(as_json: bool, all: bool) {
    let why = if all {
        "no channels on this machine yet — make one in the collab app"
    } else {
        "this chat is not subscribed to any channel yet — call collab_subscribe"
    };
    if as_json {
        emit(&serde_json::json!({"type":"status","connected":false,
            "error":why,"addr":config::addr(),"from":0})
            .to_string());
    } else {
        eprintln!("collab: {why}");
    }
}

fn report_unknown(name: &str, as_json: bool) {
    if as_json {
        emit(&serde_json::json!({"type":"status","connected":false,
            "error":format!("no key for #{name} on this machine"),
            "addr":config::addr(),"from":0})
            .to_string());
    }
}

fn watch_one(
    channel: &str,
    start: i64,
    save: bool,
    as_json: bool,
    popups: bool,
    all: bool,
    still_wanted: &dyn Fn() -> bool,
) {
    let mut warned = false;
    let mut first = true;
    let notifier = if popups {
        Notifier::new(config::name())
    } else {
        None
    };
    let addr = config::addr();
    let host = config::name();
    let cursor = std::sync::atomic::AtomicI64::new(start);

    let mut on_msg = |m: &Msg, replayed: bool| {
        use std::sync::atomic::Ordering::SeqCst;
        // A chat does not need its own words read back to it. Only this chat's
        // own are dropped — a sibling chat on the same machine is someone else,
        // and worth hearing. The place in the sequence still advances, so a
        // resume after this is still exact.
        // Both of these are about not interrupting a chat, so neither applies
        // when somebody has asked for the whole channel. `-all` is the window,
        // and a window that hides messages is not a record. The mention filter
        // was fixed for this and its twin was left behind, which is how the app
        // came to hide every message this chat had sent.
        let mine = !all
            && m.via == crate::msg::ACTOR_AI
            && m.host == host
            && config::session_name().is_some_and(|n| n == m.from);
        // A message addressed to somebody else should not interrupt this chat.
        // But `-all` is somebody asking for the whole channel — the window is
        // built from it, and a window that hides messages addressed to other
        // people is not a record of the channel. An @name narrows who is told,
        // never who may look, and dropping it here broke exactly that.
        let addressed_elsewhere = !all && !m.is_for(&config::my_names());

        // Deliver first; write down the place afterwards. The other order
        // loses messages outright: emit() exits when the reader has gone, so
        // the mark would already say "seen" for something nobody ever saw, and
        // the next run skips it for good. A watcher whose window closed between
        // receiving a message and printing it ate that message silently.
        // Filtered messages are genuinely handled, so they do advance the mark.
        if !(mine || addressed_elsewhere) {
            if as_json {
                if let Ok(s) = serde_json::to_string(
                    &serde_json::json!({"type":"msg","msg":m,"replayed":replayed}),
                ) {
                    emit(&s);
                }
            } else if replayed {
                emit(&format!(
                    "[{}] (earlier) {}: {}",
                    m.channel,
                    m.label(),
                    m.line()
                ));
            } else {
                emit(&format!("[{}] {}: {}", m.channel, m.label(), m.line()));
            }
            // Backlog is not news. Popping a notification for a message from
            // two hours ago says it has just been said, and an instruction read
            // that way gets acted on a second time.
            if let Some(n) = &notifier {
                if !replayed {
                    n.send(m);
                }
            }
        }
        cursor.store(m.seq, SeqCst);
        if save {
            save_seen_for(channel, m.seq, &mut warned);
        }
    };

    let mut on_status = |up: bool, from: i64, head: i64, err: Option<String>| {
        // How much of what is about to arrive is old, so a reader can tell
        // before it starts rather than after. Null while disconnected: we are
        // not talking to the server, so we do not know where the channel got
        // to, and 0 would read as "caught up" at exactly the wrong moment.
        let head = if up { Some(head) } else { None };
        let behind = head.map(|h| if h > from { h - from } else { 0 });
        if as_json {
            emit(
                &serde_json::json!({
                "type":"status","connected":up,"from":from,"head":head,
                "behind":behind,"addr":addr,
                "channel":channel,"error":err})
                .to_string(),
            );
            return;
        }
        if up {
            if !first {
                emit(&format!(
                    "* reconnected to {addr} #{channel}, resuming from #{from}"
                ));
            }
            first = false;
        } else {
            first = false;
            match err {
                Some(e) => emit(&format!(
                    "* DISCONNECTED from {addr} #{channel} — {e} — retrying"
                )),
                None => emit(&format!("* DISCONNECTED from {addr} #{channel} — retrying")),
            }
        }
    };

    stream_channel(channel, &cursor, still_wanted, &mut on_msg, &mut on_status);
}

/// Delivers one message on $COLLAB_CHANNEL, under this machine's own name.
pub fn send(m: Msg) -> std::io::Result<()> {
    send_full(&config::channel(), None, m)
}

/// Which channel a command was aimed at. Naming one this machine has no key for
/// is refused here: posting it to the configured channel instead would deliver
/// the message somewhere nobody asked for, and say it went fine.
fn target_channel(explicit: Option<&str>) -> String {
    match explicit.map(str::trim).filter(|s| !s.is_empty()) {
        None => config::channel(),
        Some(c) => {
            let known = channels::names();
            match known.iter().find(|k| k.eq_ignore_ascii_case(c)) {
                Some(k) => k.clone(),
                None => {
                    eprintln!(
                        "collab: no channel #{c} on this machine.\n  here: {}",
                        if known.is_empty() {
                            "none yet — make one in the Collab app".into()
                        } else {
                            known.join(", ")
                        }
                    );
                    std::process::exit(2);
                }
            }
        }
    }
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

/// Somebody who has spoken on a channel, and is therefore mentionable.
pub struct User {
    pub name: String,
    pub is_ai: bool,
    /// The machine they spoke from, when the name does not already say.
    pub host: String,
    pub messages: usize,
    pub last_at: String,
}

/// Who is on a channel. There is no register of members — a channel is a key,
/// and holding it is all it takes — so this is who has actually spoken, which
/// is also exactly who can be mentioned.
pub fn users_on(channel: &str) -> Vec<User> {
    let mut out: Vec<User> = Vec::new();
    for m in fetch(channel, 0) {
        if m.from.is_empty() {
            continue;
        }
        match out.iter_mut().find(|u| u.name == m.from) {
            Some(u) => {
                u.messages += 1;
                u.last_at = m.at.clone();
                u.is_ai = m.via == crate::msg::ACTOR_AI;
                if !m.host.is_empty() {
                    u.host = m.host.clone();
                }
            }
            None => out.push(User {
                name: m.from.clone(),
                is_ai: m.via == crate::msg::ACTOR_AI,
                host: m.host.clone(),
                messages: 1,
                last_at: m.at.clone(),
            }),
        }
    }
    out.sort_by(|a, b| b.last_at.cmp(&a.last_at)); // most recently heard from first
    out
}

pub fn users_cmd(channel: Option<&str>, all: bool) {
    let channels = if all {
        channels::names()
    } else {
        vec![channel.map(channels::tidy).unwrap_or_else(config::channel)]
    };
    for ch in channels {
        let users = users_on(&ch);
        println!("#{ch}");
        if users.is_empty() {
            println!("  nobody has spoken here yet");
            continue;
        }
        for u in users {
            let kind = if u.is_ai { "AI" } else { "Human" };
            let where_from = if u.host.is_empty() || u.host == u.name {
                String::new()
            } else {
                format!(" on {}", u.host)
            };
            let at = if u.last_at.len() >= 16 {
                &u.last_at[11..16]
            } else {
                "--:--"
            };
            println!(
                "  @{:<16} {:<6}{:<12} {} message(s), last at {at}",
                u.name, kind, where_from, u.messages
            );
        }
    }
}

/// Refuses a message whose @name reaches nobody. A misspelled mention does not
/// fail, it goes quiet — and quiet is exactly what a message nobody answered
/// looks like, so the mistake would be invisible for as long as it mattered.
///
/// Only names that have spoken on the channel count, because that is all
/// anybody here knows. Somebody set up but silent cannot be mentioned yet, so
/// the refusal lists who can be, rather than leaving the sender guessing which
/// half of the name was wrong.
pub fn mentions_reach_someone(channel: &str, text: &str) -> Result<(), String> {
    let wanted = crate::msg::mentions_in(text);
    if wanted.is_empty() {
        return Ok(()); // the common case pays nothing
    }
    let mut known: Vec<String> = config::my_names()
        .iter()
        .map(|n| n.to_lowercase())
        .collect();
    for u in users_on(channel) {
        let n = u.name.to_lowercase();
        if !known.contains(&n) {
            known.push(n);
        }
    }
    let missing: Vec<String> = wanted
        .iter()
        .filter(|w| !known.contains(w))
        .map(|w| format!("@{w}"))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut listed: Vec<String> = known.iter().map(|n| format!("@{n}")).collect();
    listed.sort();
    Err(format!(
        "{} {} nobody who has spoken on #{channel}, so that part would reach no one and \
you would not be told. Nothing was sent.\n\nNames known there: {}\n\nUse one of those, or \
drop the @ if you did not mean to address anybody. To write *about* a name rather than to it, \
put it in backticks or double the at-sign: `@name` or @@name. Somebody who has never posted \
here cannot be mentioned yet.",
        missing.join(", "),
        if missing.len() == 1 {
            "matches"
        } else {
            "match"
        },
        listed.join(", ")
    ))
}

pub fn post(text: &str, via_ai: bool, channel: Option<&str>) {
    let channel = target_channel(channel);
    let text = text.trim().replace('\n', " ");
    if text.is_empty() {
        eprintln!("usage: collab post [-c channel] \"message\"");
        std::process::exit(2);
    }
    if let Err(e) = mentions_reach_someone(&channel, &text) {
        eprintln!("collab: {e}");
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
    if let Err(e) = send_full(&channel, None, m) {
        fail(e)
    }
}

pub const CHANGE_USAGE: &str = "usage: collab change -action added|edited|removed|renamed -target \"where\" \"one-line summary\"

  -action   what you did to it
  -target   which script or instance, e.g. ServerScriptService/ShopHandler
  summary   one line, in past tense: \"gave the buy button a debounce\"";

pub fn change(action: &str, target: &str, summary: &str, via_ai: bool, channel: Option<&str>) {
    let channel = target_channel(channel);
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
    if let Err(e) = mentions_reach_someone(&channel, &summary) {
        eprintln!("collab: {e}");
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
    let Ok(mut conn) = dial(channel) else {
        return local();
    };
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

/// Sends a file. The bytes go to the store and the channel gets a reference,
/// so a screenshot is carried once rather than replayed to every watcher for
/// ever.
pub fn send_file(path: &str, caption: &str, channel: &str, via_ai: bool) -> Result<String, String> {
    let p = std::path::Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {path} — {e}"))?;
    if meta.is_dir() {
        return Err(format!("{path} is a folder; send a file"));
    }
    if meta.len() > files::MAX_BYTES {
        return Err(format!(
            "{path} is {} — the limit is {}",
            files::human(meta.len()),
            files::human(files::MAX_BYTES)
        ));
    }
    let data = std::fs::read(p).map_err(|e| format!("cannot read {path} — {e}"))?;
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".into());
    let want = files::FileRef {
        name,
        size: data.len() as u64,
        hash: files::hash_bytes(&data),
    };

    let mut conn = dial(channel).map_err(|e| e.to_string())?;
    conn.send(&Hello {
        name: config::name(),
        host: config::name(),
        channel: channel.to_string(),
        since: 0,
        mode: "put".into(),
    })
    .and_then(|_| conn.expect_welcome().map(|_| ()))
    .map_err(|e| e.to_string())?;
    conn.send(&crate::wire::FileHeader {
        file: want.clone(),
        caption: caption.to_string(),
        via: if via_ai {
            crate::msg::ACTOR_AI.into()
        } else {
            String::new()
        },
    })
    .map_err(|e| e.to_string())?;
    for chunk in data.chunks(files::CHUNK) {
        conn.send_raw(chunk).map_err(|e| e.to_string())?;
    }
    conn.send_raw(&[]).map_err(|e| e.to_string())?;

    match conn.recv::<crate::wire::Ack>() {
        Ok(Some(a)) if a.ok => Ok(a.detail),
        Ok(Some(a)) => Err(a.detail),
        _ => Err("the server did not answer".into()),
    }
}

/// Fetches a file by hash and writes it somewhere safe. The name came from
/// whoever sent it, so it is cleaned before it is ever used as a path.
pub fn get_file(
    hash: &str,
    name: &str,
    dir: &std::path::Path,
    channel: &str,
) -> Result<std::path::PathBuf, String> {
    let mut conn = dial(channel).map_err(|e| e.to_string())?;
    conn.send(&Hello {
        name: config::name(),
        host: config::name(),
        channel: channel.to_string(),
        since: 0,
        mode: "get".into(),
    })
    .and_then(|_| conn.expect_welcome().map(|_| ()))
    .map_err(|e| e.to_string())?;
    conn.send(&crate::wire::Want {
        hash: hash.to_string(),
    })
    .map_err(|e| e.to_string())?;

    match conn.recv::<crate::wire::Ack>() {
        Ok(Some(a)) if a.ok => {}
        Ok(Some(a)) => return Err(a.detail),
        _ => return Err("the server did not answer".into()),
    }
    let mut data = Vec::new();
    loop {
        match conn.recv_raw() {
            Ok(Some(chunk)) if chunk.is_empty() => break,
            Ok(Some(chunk)) => data.extend_from_slice(&chunk),
            _ => return Err("the transfer stopped part-way — nothing written".into()),
        }
    }
    // Content addressing is only worth anything if it is checked on the way in.
    if files::hash_bytes(&data) != hash {
        return Err("what arrived does not match the hash — nothing written".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let out = files::unique_path(dir, name);
    std::fs::write(&out, &data).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn default_incoming() -> std::path::PathBuf {
    config::home("Downloads").join("collab")
}

/// Every file on a channel, newest last.
pub fn files_on(channel: &str) -> Vec<Msg> {
    fetch(channel, 0)
        .into_iter()
        .filter(|m| m.is_file())
        .collect()
}

pub fn send_file_cmd(path: &str, caption: &str, channel: Option<&str>) {
    let ch = channel.map(channels::tidy).unwrap_or_else(config::channel);
    match send_file(path, caption, &ch, false) {
        Ok(detail) => println!("{detail} to #{ch}"),
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(1);
        }
    }
}

pub fn files_cmd(channel: Option<&str>) {
    let ch = channel.map(channels::tidy).unwrap_or_else(config::channel);
    let list = files_on(&ch);
    if list.is_empty() {
        println!("no files on #{ch}");
        return;
    }
    for m in list {
        if let Some(f) = &m.file {
            println!(
                "{:<10} {:<9} {}  ({})",
                &f.hash[..8.min(f.hash.len())],
                files::human(f.size),
                f.name,
                m.label()
            );
        }
    }
}

pub fn get_file_cmd(which: &str, out: Option<&str>, channel: Option<&str>) {
    let ch = channel.map(channels::tidy).unwrap_or_else(config::channel);
    let list = files_on(&ch);
    let found = list.iter().rev().find(|m| {
        m.file
            .as_ref()
            .is_some_and(|f| f.hash == which || f.hash.starts_with(which) || f.name == which)
    });
    let Some(f) = found.and_then(|m| m.file.clone()) else {
        // `files -c other` lists it and then `get` says it does not exist,
        // because get looked at the configured channel instead. Saying where
        // it actually is turns that into one command rather than a hunt.
        let elsewhere: Vec<String> = channels::names()
            .into_iter()
            .filter(|c| *c != ch)
            .filter(|c| {
                files_on(c).iter().any(|m| {
                    m.file.as_ref().is_some_and(|f| {
                        f.hash == which || f.hash.starts_with(which) || f.name == which
                    })
                })
            })
            .collect();
        eprintln!("collab: no file matching \"{which}\" on #{ch}");
        if !elsewhere.is_empty() {
            eprintln!(
                "  it is on {} — add -c {}",
                elsewhere
                    .iter()
                    .map(|c| format!("#{c}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                elsewhere[0]
            );
        }
        std::process::exit(2);
    };
    // -o naming a directory saves into it; anything else is taken as the file
    // to write. Creating a directory called `got.txt` because that is what the
    // person typed is surprising in the way that costs an afternoon.
    let (dir, rename) = match out.map(std::path::PathBuf::from) {
        None => (default_incoming(), None),
        Some(p) if p.is_dir() => (p, None),
        Some(p) if p.extension().is_some() => {
            let parent = p.parent().filter(|d| !d.as_os_str().is_empty());
            (
                parent.map(|d| d.to_path_buf()).unwrap_or_else(|| ".".into()),
                p.file_name().map(|n| n.to_string_lossy().into_owned()),
            )
        }
        Some(p) => (p, None),
    };
    match get_file(&f.hash, rename.as_deref().unwrap_or(&f.name), &dir, &ch) {
        Ok(p) => println!("saved {}", p.display()),
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(1);
        }
    }
}

/// Channels a person can see and join. Creating one is deliberately not here:
/// it happens in the app, by a person, because a key made on a whim by
/// something that cannot copy it to the other machine is a room with nobody
/// in it.
pub fn channels_cmd(show_keys: bool, as_json: bool) {
    let reg = channels::load();
    if as_json {
        // The key is only included when it was asked for. The text form has
        // always required -keys and the JSON form did not, so anything reading
        // this for a channel list — the app, a log, a screenshot of a terminal
        // — printed every key on the machine. The guard belongs on the data,
        // not on the one caller that happened to be human.
        let list: Vec<_> = reg
            .iter()
            .map(|(name, ch)| {
                let mut o = serde_json::json!({"name": name, "mine": ch.mine,
                                   "created": ch.created, "creator": ch.creator_name()});
                if show_keys {
                    o["key"] = serde_json::json!(ch.key);
                    o["invite"] = serde_json::json!(channels::invite(name, &ch.key));
                }
                o
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
            println!(
                "#{name}  ({origin})\n    invite = {}",
                channels::invite(&name, &ch.key)
            );
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
            println!("{}", channels::invite(&n, &key));
        }
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(2);
        }
    }
}

/// Takes either an invite (`name:key`, one argument) or the older
/// `<name> <key>` pair. An invite carries the name, so the person joining does
/// not choose one — which is what stopped both machines agreeing on what the
/// room was called.
pub fn channel_add(first: &str, second: &str) {
    let (name, key) = if second.trim().is_empty() {
        let (n, k) = channels::split_invite(first);
        match n {
            Some(n) => (n, k),
            None => {
                eprintln!(
                    "collab: that looks like a bare key with no channel name.\n  join with the invite the other person copied — it looks like  roblox-game:{}…\n  or give a name yourself: collab channel add <name> <key>",
                    &k.chars().take(8).collect::<String>()
                );
                std::process::exit(2);
            }
        }
    } else {
        // Explicit name wins, but an invite pasted into the key slot should not
        // smuggle its name in silently.
        (first.to_string(), channels::split_invite(second).1)
    };
    match channels::add(&name, &key, "") {
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
    if let Err(e) = conn
        .send(&hello)
        .and_then(|_| conn.expect_welcome().map(|_| ()))
    {
        eprintln!("collab: {e}");
        std::process::exit(1);
    }
    match conn.recv::<crate::wire::Ack>() {
        Ok(Some(a)) if a.ok => {
            let _ = channels::forget(&name);
            println!("{}", a.detail);
            println!(
                "the key is gone from here; anyone else still holding it can no longer connect"
            );
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
