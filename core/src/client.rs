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

/// One line, for the text stream, which is line-oriented by construction:
/// every consumer of `collab watch` treats a line as a message, including the
/// Monitors these sessions run and the greps piped after them. Emitting a real
/// newline would split one message into several, and a filter matching `^[`
/// would keep the first and drop the rest — silently, which is the failure this
/// change exists to end rather than to relocate.
///
/// So in text mode the break is *shown* instead of sent. The message arrives
/// whole, nothing downstream needs changing, and `-json` carries the real
/// newlines for anything that can hold them.
fn one_line(s: &str) -> String {
    s.replace('\n', " \u{23ce} ")
}

/// How far behind its own send time a message arrived, when that is far enough
/// to be worth saying. Thirty seconds: below that the gap is clock skew, a slow
/// disk or a person's imagination, and a note on every line would be noise
/// nobody reads — which is the same as no note at all.
///
/// `None` when it is prompt, when the clocks disagree in the impossible
/// direction, or when the timestamp will not parse. A missing note means
/// nothing was measured, not that nothing was wrong.
fn lateness(m: &Msg) -> Option<std::time::Duration> {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    let sent = OffsetDateTime::parse(&m.at, &Rfc3339).ok()?;
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let secs = (now - sent).whole_seconds();
    (secs >= 30).then(|| std::time::Duration::from_secs(secs as u64))
}

/// Plain words for a gap. "arrived 4m late" is a fact somebody can act on;
/// "arrived 247s late" is arithmetic homework.
fn human_gap(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s < 90 {
        format!("{s}s")
    } else if s < 5400 {
        format!("{}m", (s + 30) / 60)
    } else {
        format!("{}h{}m", s / 3600, (s % 3600) / 60)
    }
}

/// Where one chat's place in one channel is recorded.
///
/// It used to be the channel name alone, in a file every session on the machine
/// shares — so four watchers wrote one number per channel and whichever was
/// furthest ahead set the resume point for all of them. Every reconnect reads
/// this, so a watcher could resume from another session's position and skip
/// what lay between: silently, permanently, and looking exactly like a message
/// that was never sent. A four-minute delay and a message lost forever were the
/// same mechanism, decided by which watcher happened to write last.
///
/// A terminal with no session keeps the bare channel name, which is also what
/// makes the old entries readable below.
fn seen_key(channel: &str) -> String {
    let id = config::session_id();
    if id.is_empty() {
        channel.to_string()
    } else {
        format!("{id}\u{1}{channel}")
    }
}

pub fn seen_for(channel: &str) -> i64 {
    let map = seen_map();
    if let Some(n) = map.get(&seen_key(channel)) {
        return *n;
    }
    // First read after the split: inherit the machine's old shared place rather
    // than replaying the channel from the beginning. Wrong by at most whatever
    // another session had already read, and right about not dumping three days
    // of history into somebody's window.
    if let Some(n) = map.get(channel) {
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
/// One watcher runs a thread per channel and they all write this file. The
/// read-modify-write has to be one operation or two threads racing lose each
/// other's places.
static SEEN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save_seen_for(channel: &str, n: i64, warned: &mut bool) {
    let _guard = SEEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = seen_map();
    map.insert(seen_key(channel), n);
    let path = config::home(".collab-seen.json");
    // Written through a rename. std::fs::write truncates first, and seen_map()
    // reads an unparseable file as an empty map — so a thread reading during
    // another's write would find nothing, then save only its own channel and
    // destroy every other marker. That is not a stale place in the sequence,
    // it is a full replay of every other channel, and it happened twice
    // tonight on two different machines.
    // One temp name per process, not one for the machine. The lock above is a
    // `static` — it orders threads inside one process and cannot see the other
    // watchers, and there are usually several. They all staged through the same
    // scratch file, so one process's rename pulled the file out from under
    // another's, whose rename then failed and reported it could not record its
    // place. Measured before the fix: six concurrent writers, ~150 failures each
    // out of 300. After: zero.
    let tmp = config::home(&format!(".collab-seen.json.{}.tmp", std::process::id()));
    let ok = serde_json::to_string(&map)
        .ok()
        .and_then(|t| std::fs::write(&tmp, t).ok())
        .and_then(|_| std::fs::rename(&tmp, &path).ok())
        .is_some();
    if ok {
        config::lock_down(&path);
    } else {
        let _ = std::fs::remove_file(&tmp);
        if !*warned {
            *warned = true;
            eprintln!(
                "* cannot record my place in {} — after a reconnect you may see old messages again",
                path.display()
            );
        }
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

/// Connects, remembering what the name resolved to last time.
///
/// A `.local` name is the right thing to put in a config — addresses change
/// every time a router hands out a new lease, and this one changed twice in a
/// week. But resolving it costs about two seconds per connection on Windows,
/// every connection, because each `collab` run is a new process with nothing
/// cached. Measured from the VM: 2.3s by name against 0.22s by address, and a
/// message carrying an `@` opens two connections, so it paid twice.
///
/// So the name stays in the config and the address it resolved to is kept
/// beside it. The cache is a hint, never the truth: if connecting to it fails
/// the name is resolved again, which is exactly the case a fresh lease creates.
fn dial(channel: &str) -> std::io::Result<Conn> {
    let addr = config::addr();
    if let Some(cached) = resolved_addr(&addr) {
        if let Ok(stream) = TcpStream::connect(&cached) {
            let _ = stream.set_nodelay(true);
            return Conn::connect(stream, channel);
        }
        forget_resolved(&addr); // the lease moved, or the machine did
    }
    let stream = TcpStream::connect(&addr)?;
    let _ = stream.set_nodelay(true);
    if let Ok(peer) = stream.peer_addr() {
        remember_resolved(&addr, &peer.to_string());
    }
    let _ = stream.set_nodelay(true);
    Conn::connect(stream, channel)
}

/// The address a name last resolved to, if it was recent enough to trust.
/// An hour: long enough that a burst of messages pays nothing, short enough
/// that a machine which moved is found again without anyone intervening.
fn resolved_addr(name: &str) -> Option<String> {
    if name.parse::<std::net::SocketAddr>().is_ok() {
        return None; // already an address; nothing to resolve or remember
    }
    let raw = std::fs::read_to_string(config::home(".collab-resolved.json")).ok()?;
    let map: std::collections::BTreeMap<String, (String, u64)> =
        serde_json::from_str(&raw).ok()?;
    let (addr, when) = map.get(name)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    (now.saturating_sub(*when) < 3600).then(|| addr.clone())
}

fn remember_resolved(name: &str, addr: &str) {
    if name.parse::<std::net::SocketAddr>().is_ok() || name == addr {
        return;
    }
    let path = config::home(".collab-resolved.json");
    let mut map: std::collections::BTreeMap<String, (String, u64)> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    map.insert(name.to_string(), (addr.to_string(), now));
    if let Ok(text) = serde_json::to_string(&map) {
        let _ = std::fs::write(&path, text);
    }
}

fn forget_resolved(name: &str) {
    let path = config::home(".collab-resolved.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut map) = serde_json::from_str::<std::collections::BTreeMap<String, (String, u64)>>(&raw)
    else {
        return;
    };
    map.remove(name);
    if let Ok(text) = serde_json::to_string(&map) {
        let _ = std::fs::write(&path, text);
    }
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
        let addressed_elsewhere = !all && !m.is_for(&config::my_names_on(&m.channel));

        // Deliver first; write down the place afterwards. The other order
        // loses messages outright: emit() exits when the reader has gone, so
        // the mark would already say "seen" for something nobody ever saw, and
        // the next run skips it for good. A watcher whose window closed between
        // receiving a message and printing it ate that message silently.
        // Filtered messages are genuinely handled, so they do advance the mark.
        if !(mine || addressed_elsewhere) {
            // How long it took to arrive. A stream event carries when a
            // message was *sent* and nothing about when it landed, so from
            // inside a session "was that late?" has no answer — and this
            // morning four of us spent an hour on a message that turned out to
            // be four minutes late rather than lost. "Not yet" and "never" are
            // the same observation until something says which.
            //
            // Silent when it is prompt, which is nearly always. A line that
            // appears only when it has something to say is worth reading.
            let late = lateness(m);
            if as_json {
                if let Ok(s) = serde_json::to_string(&serde_json::json!({
                    "type":"msg","msg":m,"replayed":replayed,
                    "late_seconds": late.map(|d| d.as_secs()),
                })) {
                    emit(&s);
                }
            } else {
                let note = match late {
                    Some(d) => format!(" (arrived {} late)", human_gap(d)),
                    None => String::new(),
                };
                if replayed {
                    emit(&format!(
                        "[{}] (earlier) {}: {}{}",
                        m.channel,
                        m.label(),
                        one_line(&m.line()),
                        note
                    ));
                } else {
                    emit(&format!(
                        "[{}] {}: {}{}",
                        m.channel,
                        m.label(),
                        one_line(&m.line()),
                        note
                    ));
                }
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
    let ch = config::channel();
    send_full(&ch, channels::display_for(&ch).as_deref(), m)
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
    for m in speakers(channel) {
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
            // The spellable form. This list is read to find out what to type,
            // and a name with a gap in it cannot be written as a mention.
            let name = crate::msg::addressable(&u.name);
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
                name, kind, where_from, u.messages
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
/// What to say after a post that named somebody, or nothing when it named
/// nobody. An `@` narrows *delivery*, not merely emphasis: a watcher never
/// emits a message addressed elsewhere, so from the sender's side addressing
/// three people and addressing the whole channel look identical — both say
/// "sent". Two sessions paid for that today, each posting a status update to
/// one name and assuming the rest had seen it.
///
/// The message is still on the channel and still comes back from the window,
/// the log and `collab_recent`. It is the *stream* it stays out of, which is
/// the only place an AI session finds out about anything unasked.
pub fn reach_note_from(channel: &str, text: &str, people: &[User]) -> Option<String> {
    let wanted = crate::msg::mentions_in(text);
    if wanted.is_empty() {
        return None;
    }
    // Backticked, because this note is quoted. The natural way to discuss a
    // delivery report is to paste the list it just handed you — and a bare
    // `@name` in that paste is a mention, so quoting "who will not see this"
    // silently delivers to exactly them. Written this once and did it twice.
    let named: Vec<String> = wanted.iter().map(|w| format!("`@{w}`")).collect();
    // Not the sender. A watcher drops what its own session sent, so the person
    // posting was never going to see it in their stream and saying they will not
    // is noise dressed as a warning.
    let mine: Vec<String> = config::my_names_on(channel)
        .iter()
        .map(|n| crate::msg::addressable(n))
        .collect();
    let others: Vec<String> = people
        .iter()
        .map(|u| crate::msg::addressable(&u.name))
        .filter(|n| !wanted.contains(n) && !mine.contains(n))
        .map(|n| format!("`@{n}`"))
        .collect();
    if others.is_empty() {
        return None; // it named everyone there; nothing was narrowed
    }
    let mut rest = others;
    rest.sort();
    rest.dedup();
    Some(format!(
        "delivered to {} only. {} will not see it in their watcher — it is on \
         #{channel} and readable, but nothing will tell them it is there. \
         For something everyone should see, name nobody.",
        named.join(", "),
        rest.join(", ")
    ))
}

/// The mention check and the delivery note, from one look at the channel.
///
/// Both need to know who has spoken there, and `users_on` is not cheap: it
/// dials the server and pulls the channel's whole history. Calling it twice
/// doubled the cost of every post carrying an `@`, on the Windows GUI's UI
/// thread, which is a freeze rather than a slowdown — Tankun hit it the same
/// afternoon the note was added.
pub fn mention_check(channel: &str, text: &str) -> Result<Option<String>, String> {
    if crate::msg::mentions_in(text).is_empty() {
        return Ok(None); // the common case still pays nothing
    }
    let people = users_on(channel);
    mentions_reach_someone_with(channel, text, &people)?;
    Ok(reach_note_from(channel, text, &people))
}

pub fn mentions_reach_someone(channel: &str, text: &str) -> Result<(), String> {
    let wanted = crate::msg::mentions_in(text);
    if wanted.is_empty() {
        return Ok(());
    }
    let people = users_on(channel);
    mentions_reach_someone_with(channel, text, &people)
}

fn mentions_reach_someone_with(channel: &str, text: &str, people: &[User]) -> Result<(), String> {
    let wanted = crate::msg::mentions_in(text);
    if wanted.is_empty() {
        return Ok(()); // the common case pays nothing
    }
    let mut known: Vec<String> = config::my_names_on(channel)
        .iter()
        .map(|n| crate::msg::addressable(n))
        .collect();
    for u in people {
        let n = crate::msg::addressable(&u.name);
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
    let text = text.trim().to_string();
    if text.is_empty() {
        eprintln!("usage: collab post [-c channel] \"message\"");
        std::process::exit(2);
    }
    let note = match mention_check(&channel, &text) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(2);
        }
    };
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
    if let Err(e) = send_full(&channel, channels::display_for(&channel).as_deref(), m) {
        fail(e)
    }
    if let Some(n) = note {
        eprintln!("collab: {n}");
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
    let summary = summary.trim().to_string();
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
/// One message per distinct speaker, from the server, instead of the whole
/// channel. Falls back to a full fetch when the server does not know the mode —
/// an older one closes without answering, and an empty answer is
/// indistinguishable from a channel nobody has spoken on.
fn speakers(channel: &str) -> Vec<Msg> {
    let Ok(mut conn) = dial(channel) else {
        return history::filter(history::read(), channel, 0);
    };
    let hello = Hello {
        name: config::name(),
        host: config::name(),
        channel: channel.to_string(),
        since: 0,
        mode: "who".into(),
    };
    if conn
        .send(&hello)
        .and_then(|_| conn.expect_welcome().map(|_| ()))
        .is_err()
    {
        return fetch(channel, 0);
    }
    let mut out = Vec::new();
    while let Ok(Some(m)) = conn.recv::<Msg>() {
        out.push(m);
    }
    if out.is_empty() {
        // Either nobody has spoken, or the server is old enough not to know
        // "who". The full fetch answers both without guessing which.
        return fetch(channel, 0);
    }
    out
}

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
                o["display"] = serde_json::json!(ch.display);
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

/// Prints the same two lines as `channel_create` — name, then invite — because
/// both GUIs read it the same way, and because a new key is useless until the
/// other person has it.
pub fn channel_rotate(name: &str) {
    match channels::rotate(name) {
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

/// What to call yourself on one channel. The machine name is nobody's choice —
/// it is whatever the computer was called when it was set up — and the same
/// person is reasonably a different name to their family and to a work project.
pub fn channel_name(channel: &str, display: &str) {
    if channel.is_empty() {
        eprintln!("usage: collab channel name <channel> <what to call yourself>");
        std::process::exit(2);
    }
    match channels::set_display(channel, display) {
        Ok(()) => {
            let ch = channels::tidy(channel);
            match channels::display_for(&ch) {
                Some(d) => println!("you are \"{d}\" on #{ch}"),
                None => println!(
                    "cleared — you are \"{}\" on #{ch} again, which is this machine's name",
                    config::name()
                ),
            }
        }
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
