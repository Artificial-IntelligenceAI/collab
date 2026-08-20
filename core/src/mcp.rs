//! The MCP side. A server cannot push anything into a session, so this
//! deliberately offers only tools — pulling. The push still comes from `collab
//! watch` running under a Monitor.
//!
//! Tested 2026-08-19: a server pushed 25 notifications over 8 minutes via both
//! notifications/resources/updated and notifications/message, and the client
//! never subscribed and never reacted. Advertising a capability that does
//! nothing would be a lie in the handshake, so we advertise tools and nothing else.
use crate::channels;
use crate::client;
use crate::config;
use crate::msg::{Msg, ACTIONS, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn tools() -> Value {
    json!([
      {
        "name": "collab_join",
        "description": "Say who you are and which channels you are listening to, for this chat only. \
Required before you can post anything — collab_post and collab_change refuse until you have. \
Call it once, early. The name is yours to pick: something short saying which part of the project \
this chat is working on (\"shop\", \"lobby-audio\"). The channels are not yours to pick — they have \
to be ones this machine already holds keys for, made by a person in the collab app. Call \
collab_recent first if you do not know which exist. You may listen to several at once; use \
collab_subscribe afterwards to change the set.",
        "inputSchema": {"type":"object","properties":{
            "name":     {"type":"string","description":"Short name for this chat, e.g. \"shop\"."},
            "channels": {"type":"array","items":{"type":"string"},
                         "description":"Channels to listen to. One is normal; several is allowed."}},
          "required":["name","channels"]}
      },
      {
        "name": "collab_subscribe",
        "description": "Change which channels this chat is listening to. Replaces the whole set, so \
pass every channel you want, not only the new ones. Passing an empty list stops listening \
altogether. Messages on a channel you are not subscribed to will not reach you at all — that is \
the point of it, but it also means a channel you drop goes silent rather than telling you \
anything.",
        "inputSchema": {"type":"object","properties":{
            "channels": {"type":"array","items":{"type":"string"},
                         "description":"The complete set of channels to listen to."}},
          "required":["channels"]}
      },
      {
        "name": "collab_post",
        "description": "Send a chat message to the other person's Claude. Use it to say what you are \
about to touch, to ask them something, or to answer them. For recording something you actually \
changed, use collab_change instead. Requires collab_join first. If you are listening to more \
than one channel you must say which one this goes to.",
        "inputSchema": {"type":"object","properties":{
            "message": {"type":"string","description":"What to tell them."},
            "channel": {"type":"string","description":"Which channel. Only optional when you are listening to exactly one."}},
          "required":["message"]}
      },
      {
        "name": "collab_change",
        "description": "Record something you just changed, as a structured entry. This is what fills \
the Changes view — a git log for a project that cannot use git, because Roblox saves a binary \
.rbxl. Call it right after you make a change, once per script or instance you touched. Only \
record what you actually did; never infer an entry from what someone said. Requires collab_join \
first. If you are listening to more than one channel you must say which one this belongs to.",
        "inputSchema": {"type":"object","properties":{
            "action":  {"type":"string","enum":ACTIONS,"description":"What you did: added, edited, removed or renamed."},
            "target":  {"type":"string","description":"Which script or instance, as a path — e.g. ServerScriptService/ShopHandler."},
            "summary": {"type":"string","description":"One line, past tense, what changed."},
            "channel": {"type":"string","description":"Which channel. Only optional when you are listening to exactly one."}},
          "required":["action","target","summary"]}
      },
      {
        "name": "collab_recent",
        "description": "Read recent activity on the shared channel, oldest first — both chat and recorded changes.",
        "inputSchema": {"type":"object","properties":{
            "count":   {"type":"integer","description":"How many entries (default 20)."},
            "kind":    {"type":"string","enum":["all","chat","change"],"description":"Which kind to show (default all)."},
            "channel": {"type":"string","description":"One channel. Omit for everything you are listening to, or every channel here if you have not subscribed."}}}
      },
      {
        "name": "collab_changes",
        "description": "Read the recorded changes on the shared channel, newest first, grouped by who made them. \
    Read this before touching a script, to see if the other session has already been in it.",
        "inputSchema": {"type":"object","properties":{
            "count":   {"type":"integer","description":"How many changes (default 20)."},
            "channel": {"type":"string","description":"One channel. Omit for everything you are listening to."}}}
      }
    ])
}

/// Refusing is the point. An unnamed chat posts as the machine, and every other
/// chat on that machine posts as the machine too — so the other person sees one
/// voice doing contradictory things and cannot tell which of them to ask.
/// Refusing is the point. An unjoined chat posts as the machine, onto the
/// machine's default channel — so every chat on it becomes one voice on one
/// heap, which is the mess this exists to prevent.
fn needs_join() -> String {
    format!(
        "REFUSED: this chat has not joined yet. Call collab_join with a name and the channels \
to listen to, then send this again. Nothing was posted.\n\nChannels available here: {}\n\nThey \
must be from that list. A channel only works if both machines hold its key, so one that is not \
listed cannot be reached from here at all.",
        channel_list()
    )
}

/// Every requested channel must be one this machine holds a key for. All or
/// nothing: a partial subscription would leave a chat believing it was listening
/// somewhere it was not.
fn subscribe_to(wanted: &[String]) -> Result<Vec<String>, String> {
    let missing: Vec<&String> = wanted.iter().filter(|c| channels::get(c).is_none()).collect();
    if !missing.is_empty() {
        return Err(format!(
            "no channel{} {} on this machine, so there is no key and nothing to listen to. \
Channels are made by a person, with the button in the collab app — an AI cannot make one, \
because a key that has not been handed to the other machine is a room with nobody in \
it.\n\nAvailable here: {}",
            if missing.len() == 1 { "" } else { "s" },
            missing.iter().map(|c| format!("#{c}")).collect::<Vec<_>>().join(", "),
            channel_list()
        ));
    }
    let mut out = wanted.to_vec();
    out.sort();
    out.dedup();
    Ok(out)
}

fn describe(chans: &[String]) -> String {
    let mut parts = Vec::new();
    for c in chans {
        let n = client::fetch(c, 0).len();
        parts.push(format!("#{c}: {n} message(s)"));
    }
    parts.join(", ")
}

fn channel_list() -> String {
    let names = channels::names();
    if names.is_empty() {
        "none yet — a person has to make one in the collab app".to_string()
    } else {
        names.join(", ")
    }
}

fn text(s: String) -> Value {
    json!({"content":[{"type":"text","text": s}]})
}

pub fn run() {
    // One `collab mcp` process is spawned per chat, so a name held here is a
    // per-chat identity by construction — nothing to register, nothing to expire.
    let mut session_name: Option<String> = None;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(req): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let result: Option<Value> = match method {
            "initialize" => {
                let ver = req
                    .pointer("/params/protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("2025-06-18");
                Some(json!({
                    "protocolVersion": ver,
                    // No resources, no subscriptions — they demonstrably do nothing.
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name":"collab","version":"3.0.0"},
                    "instructions": "Call collab_join once, before posting anything, with a name for this \
chat and the channel the project uses. Until you do, collab_post and collab_change refuse. The \
name keeps this chat apart from other chats on the same machine, which would otherwise all be \
one indistinguishable voice. The channel keeps this project apart from every other one on the \
machine — but it must match what the other person is on, so look at collab_recent or \
collab_changes and join a channel that already has traffic rather than inventing one. A \
mismatched channel is not an error, it is silence."
                }))
            }
            "tools/list" => Some(json!({"tools": tools()})),
            "tools/call" => Some(call(&req, &mut session_name)),
            "resources/list" => Some(json!({"resources": []})),
            "prompts/list" => Some(json!({"prompts": []})),
            "ping" => Some(json!({})),
            _ => None,
        };

        let Some(id) = id else { continue }; // a notification wants no reply
        let reply = match result {
            Some(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            None => json!({"jsonrpc":"2.0","id":id,
                           "error":{"code":-32601,"message":format!("no method {method}")}}),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}

/// A list argument, tolerating a bare string for a single item.
/// What this chat is, taking the file as the truth and in-process memory only
/// as a fallback. The MCP server can be restarted under a chat that has already
/// joined, and telling it to join again when it plainly has would be a lie.
fn joined(live: &Option<String>) -> (Option<String>, Vec<String>) {
    if !config::session_id().is_empty() {
        if let Some(s) = config::session() {
            let name = Some(s.name.clone()).filter(|n| !n.is_empty());
            if name.is_some() {
                return (name, s.listening());
            }
        }
    }
    (live.clone(), config::session_channels())
}

fn arg_list(req: &Value, k: &str) -> Vec<String> {
    match req.pointer(&format!("/params/arguments/{k}")) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str())
            .map(channels::tidy)
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => vec![channels::tidy(s)].into_iter().filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    }
}

/// The name this chat posts under — from the file, so a restarted MCP server
/// keeps the identity the chat already chose rather than quietly reverting to
/// the machine's own name.
fn posting_name(live: &Option<String>) -> Option<String> {
    joined(live).0
}

/// Which channel a post belongs to. Listening to several is allowed; guessing
/// between them is not — a message in the wrong room is worse than a refusal,
/// because nobody finds out.
fn post_target(req: &Value, live: &Option<String>) -> Result<String, String> {
    let (name, subs) = joined(live);
    if name.is_none() {
        return Err(needs_join());
    }
    let asked = channels::tidy(arg(req, "channel"));
    if !asked.is_empty() {
        if !subs.contains(&asked) {
            return Err(format!(
                "not listening to #{asked}, so nothing was sent. This chat is listening to: {}. \
Use collab_subscribe to add it, or post to one of those.",
                if subs.is_empty() { "nothing".into() } else { subs.join(", ") }
            ));
        }
        return Ok(asked);
    }
    match subs.len() {
        0 => Err("this chat is not listening to any channel — call collab_subscribe first. \
Nothing was sent."
            .into()),
        1 => Ok(subs[0].clone()),
        _ => Err(format!(
            "this chat is listening to several channels ({}), so it is not clear where this \
should go and nothing was sent. Say which one.",
            subs.join(", ")
        )),
    }
}

/// What a read covers: one named channel, or everything being listened to.
fn read_scope(req: &Value) -> Vec<String> {
    let asked = channels::tidy(arg(req, "channel"));
    if !asked.is_empty() {
        return vec![asked];
    }
    let subs = config::session_channels();
    if subs.is_empty() {
        channels::names()
    } else {
        subs
    }
}

fn fetch_scope(req: &Value) -> Vec<Msg> {
    let mut all: Vec<Msg> = read_scope(req)
        .iter()
        .flat_map(|c| client::fetch(c, 0))
        .collect();
    all.sort_by_key(|m| m.seq);
    all.dedup_by_key(|m| m.seq);
    all
}

fn arg<'a>(req: &'a Value, k: &str) -> &'a str {
    req.pointer(&format!("/params/arguments/{k}"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn call(req: &Value, session_name: &mut Option<String>) -> Value {
    let name = req
        .pointer("/params/name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let count = req
        .pointer("/params/arguments/count")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(20) as usize;

    match name {
        "collab_join" => {
            let chosen: String = arg(req, "name")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(24)
                .collect();
            if chosen.is_empty() {
                return text("a join needs a name".into());
            }
            let wanted = arg_list(req, "channels");
            if wanted.is_empty() {
                return text(format!(
                    "a join needs at least one channel to listen to.\n\nAvailable here: {}",
                    channel_list()
                ));
            }
            match subscribe_to(&wanted) {
                Err(e) => text(e),
                Ok(ok) => {
                    *session_name = Some(chosen.clone());
                    config::save_session(&chosen, &ok);
                    text(format!(
                        "you are \"{chosen}\" on {} for this chat (on {}). {}",
                        ok.iter().map(|c| format!("#{c}")).collect::<Vec<_>>().join(", "),
                        config::name(),
                        describe(&ok)
                    ))
                }
            }
        }
        "collab_subscribe" => {
            let (Some(name), _) = joined(session_name) else {
                return text(needs_join());
            };
            let wanted = arg_list(req, "channels");
            if wanted.is_empty() {
                config::save_session(&name, &[]);
                return text(
                    "listening to nothing now. Messages will not reach this chat at all until \
you subscribe again."
                        .into(),
                );
            }
            match subscribe_to(&wanted) {
                Err(e) => text(e),
                Ok(ok) => {
                    config::save_session(&name, &ok);
                    text(format!(
                        "listening to {}. {}",
                        ok.iter().map(|c| format!("#{c}")).collect::<Vec<_>>().join(", "),
                        describe(&ok)
                    ))
                }
            }
        }
        "collab_post" => {
            let m = arg(req, "message").trim().replace('\n', " ");
            if m.is_empty() {
                return text("nothing to send — message was empty".into());
            }
            let to = match post_target(req, session_name) {
                Ok(c) => c,
                Err(e) => return text(e),
            };
            match client::send_full(
                &to,
                posting_name(session_name).as_deref(),
                Msg { kind: KIND_CHAT.into(), via: ACTOR_AI.into(), text: m.clone(), ..Default::default() },
            ) {
                Ok(()) => text(format!("sent: {m}")),
                Err(e) => text(format!(
                    "could not reach the collab server at {} ({e}) — the other session did NOT get this",
                    config::addr())),
            }
        }
        "collab_change" => {
            let action = arg(req, "action").trim().to_lowercase();
            let target = arg(req, "target").trim().to_string();
            let summary = arg(req, "summary").trim().replace('\n', " ");
            if !ACTIONS.contains(&action.as_str()) {
                return text(format!("action must be one of: {}", ACTIONS.join(", ")));
            }
            if target.is_empty() || summary.is_empty() {
                return text("a change needs both a target and a one-line summary".into());
            }
            let m = Msg {
                kind: KIND_CHANGE.into(),
                via: ACTOR_AI.into(),
                action: action.clone(),
                target: target.clone(),
                text: summary.clone(),
                ..Default::default()
            };
            let to = match post_target(req, session_name) {
                Ok(c) => c,
                Err(e) => return text(e),
            };
            match client::send_full(&to, posting_name(session_name).as_deref(), m) {
                Ok(()) => text(format!("recorded: {action} {target} — {summary}")),
                Err(e) => text(format!(
                    "could not reach the collab server at {} ({e}) — the change was NOT recorded",
                    config::addr()
                )),
            }
        }
        "collab_recent" => {
            let kind = arg(req, "kind");
            let mut h = fetch_scope(req);
            if kind == "chat" || kind == "change" {
                h.retain(|m| m.kind() == kind);
            }
            if h.len() > count {
                h = h.split_off(h.len() - count);
            }
            if h.is_empty() {
                return text("(nothing on this channel yet)".into());
            }
            text(
                h.iter()
                    .map(|m| format!("#{} {}: {}", m.seq, m.label(), m.line()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        "collab_changes" => {
            let mut ch: Vec<Msg> = fetch_scope(req)
                .into_iter()
                .filter(|m| m.is_change())
                .collect();
            if ch.len() > count {
                ch = ch.split_off(ch.len() - count);
            }
            ch.reverse(); // newest first, like git log
            if ch.is_empty() {
                return text("(no changes recorded yet)".into());
            }
            let mut out = String::new();
            let mut who = String::new();
            for m in &ch {
                if m.label() != who {
                    who = m.label();
                    let at = if m.at.len() >= 16 {
                        m.at[..16].replace('T', " ")
                    } else {
                        m.at.clone()
                    };
                    out.push_str(&format!("\n{who} — {at}\n"));
                }
                out.push_str(&format!("  {:<8} {} — {}\n", m.action, m.target, m.text));
            }
            text(out.trim_start_matches('\n').to_string())
        }
        other => text(format!("unknown tool {other}")),
    }
}
