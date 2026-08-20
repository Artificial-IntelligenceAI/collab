//! The MCP side. A server cannot push anything into a session, so this
//! deliberately offers only tools — pulling. The push still comes from `collab
//! watch` running under a Monitor.
//!
//! Tested 2026-08-19: a server pushed 25 notifications over 8 minutes via both
//! notifications/resources/updated and notifications/message, and the client
//! never subscribed and never reacted. Advertising a capability that does
//! nothing would be a lie in the handshake, so we advertise tools and nothing else.
use crate::client;
use crate::config;
use crate::msg::{Msg, ACTIONS, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn tools() -> Value {
    json!([
      {
        "name": "collab_join",
        "description": "Join a channel under a name, for this chat only. Required before you can post \
anything at all — collab_post and collab_change refuse until you have. Call it once, early. \
The name is yours to pick: something short saying which part of the project this chat is working \
on (\"shop\", \"lobby-audio\"), so the other person can tell your messages apart from another chat \
on the same machine. The channel is NOT yours to pick — it has to be the one the other person is \
already on, or you will both be talking into silence and neither of you will see anything wrong. \
Call collab_recent or collab_changes first to see which channels have traffic, and join one of \
those. Only invent a channel name if you have been told to.",
        "inputSchema": {"type":"object","properties":{
            "name":    {"type":"string","description":"Short name for this chat, e.g. \"shop\"."},
            "channel": {"type":"string","description":"The channel the project already uses. Must match the other machine exactly."}},
          "required":["name","channel"]}
      },
      {
        "name": "collab_post",
        "description": "Send a chat message to the other person's Claude on the shared channel. \
    Use it to say what you are about to touch, to ask them something, or to answer them. \
    For recording something you actually changed, use collab_change instead. \
Requires collab_set_name to have been called in this chat first; without it this is refused.",
        "inputSchema": {"type":"object","properties":{
            "message": {"type":"string","description":"What to tell them."}},
          "required":["message"]}
      },
      {
        "name": "collab_change",
        "description": "Record something you just changed, as a structured entry. This is what fills the \
    Changes view — a git log for a project that cannot use git, because Roblox saves a binary .rbxl. \
    Call it right after you make a change, once per script or instance you touched. \
    Only record what you actually did; never infer an entry from what someone said. \
Requires collab_set_name to have been called in this chat first; without it this is refused.",
        "inputSchema": {"type":"object","properties":{
            "action": {"type":"string","enum": ACTIONS, "description":"What you did: added, edited, removed or renamed."},
            "target": {"type":"string","description":"Which script or instance, as a path — e.g. ServerScriptService/ShopHandler."},
            "summary":{"type":"string","description":"One line, past tense, what changed."}},
          "required":["action","target","summary"]}
      },
      {
        "name": "collab_recent",
        "description": "Read recent activity on the shared channel, oldest first — both chat and recorded changes.",
        "inputSchema": {"type":"object","properties":{
            "count": {"type":"integer","description":"How many entries (default 20)."},
            "kind":  {"type":"string","enum":["all","chat","change"],"description":"Which kind to show (default all)."}}}
      },
      {
        "name": "collab_changes",
        "description": "Read the recorded changes on the shared channel, newest first, grouped by who made them. \
    Read this before touching a script, to see if the other session has already been in it.",
        "inputSchema": {"type":"object","properties":{
            "count": {"type":"integer","description":"How many changes (default 20)."}}}
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
    let mut seen: Vec<String> = Vec::new();
    for m in client::fetch("", 0) {
        if !m.channel.is_empty() && !seen.contains(&m.channel) {
            seen.push(m.channel.clone());
        }
    }
    let existing = if seen.is_empty() {
        "none yet — this would be the first".to_string()
    } else {
        seen.join(", ")
    };
    format!(
        "REFUSED: this chat has not joined a channel yet. Call collab_join with a name and a \
channel, then send this again. Nothing was posted.\n\nChannels already in use: {existing}\nThis \
machine's default: {}\n\nJoin an existing channel rather than inventing one — a channel only \
works if it matches the other machine exactly, and a mismatch looks like silence rather than \
an error.",
        config::channel()
    )
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
            let chan: String = arg(req, "channel")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
                .chars()
                .take(32)
                .collect();
            if chosen.is_empty() || chan.is_empty() {
                return text("a join needs both a name and a channel".into());
            }
            *session_name = Some(chosen.clone());
            // Written down so this chat's own watcher follows the channel it
            // joined, and recognises its own messages coming back.
            config::save_session(&chosen, &chan);

            let here = client::fetch(&chan, 0);
            let summary = match here.last() {
                Some(last) => format!(
                    "{} message{} already here, the last from {}",
                    here.len(),
                    if here.len() == 1 { "" } else { "s" },
                    last.label()
                ),
                None => "nothing here yet — you are the first".to_string(),
            };
            text(format!(
                "you are \"{chosen}\" on #{chan} for this chat (on {}). {summary}",
                config::name()
            ))
        }
        "collab_post" => {
            let Some(_) = session_name.as_deref() else {
                return text(needs_join());
            };
            let m = arg(req, "message").trim().replace('\n', " ");
            if m.is_empty() {
                return text("nothing to send — message was empty".into());
            }
            match client::send_full(
                &config::session_channel(),
                session_name.as_deref(),
                Msg { kind: KIND_CHAT.into(), via: ACTOR_AI.into(), text: m.clone(), ..Default::default() },
            ) {
                Ok(()) => text(format!("sent: {m}")),
                Err(e) => text(format!(
                    "could not reach the collab server at {} ({e}) — the other session did NOT get this",
                    config::addr())),
            }
        }
        "collab_change" => {
            let Some(_) = session_name.as_deref() else {
                return text(needs_join());
            };
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
            match client::send_full(&config::session_channel(), session_name.as_deref(), m) {
                Ok(()) => text(format!("recorded: {action} {target} — {summary}")),
                Err(e) => text(format!(
                    "could not reach the collab server at {} ({e}) — the change was NOT recorded",
                    config::addr()
                )),
            }
        }
        "collab_recent" => {
            let kind = arg(req, "kind");
            let mut h = client::fetch(&config::session_channel(), 0);
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
            let mut ch: Vec<Msg> = client::fetch(&config::session_channel(), 0)
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
