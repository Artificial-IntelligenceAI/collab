//! Real OS notifications, raised by the platform helper.
//!
//! Neither platform lets a plain binary do this: macOS attributes a
//! notification to an application bundle and Windows to a registered
//! AppUserModelID, so the popup comes from Collab.app or collab-notify.exe. If
//! neither is present collab stays quiet rather than falling back to something
//! that pops up under another app's name.
use crate::config;
use crate::msg::Msg;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::time::Duration;

/// A machine waking from sleep is replayed everything it missed at once, and
/// forty popups in a row is not a notification, it is a punishment. Arrivals
/// are collected until the channel goes quiet.
const QUIET: Duration = Duration::from_millis(700);

pub fn find_notifier() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe.parent()?.to_path_buf();

    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        for base in [
            dir.clone(),
            config::home("Applications"),
            PathBuf::from("/Applications"),
        ] {
            candidates.push(base.join("Collab.app/Contents/MacOS/collab-notify"));
            candidates.push(base.join("collab.app/Contents/MacOS/collab-notify"));
        }
    } else if cfg!(target_os = "windows") {
        candidates.push(dir.join("collab-notify.exe"));
        candidates.push(dir.join("notify/collab-notify.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn gui_url() -> String {
    format!(
        "http://127.0.0.1:{}",
        config::env("COLLAB_GUI_PORT", "8788")
    )
}

fn ellipsis(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    format!("{}…", chars[..n - 1].iter().collect::<String>().trim_end())
}

pub struct Notifier {
    tx: SyncSender<Msg>,
}

impl Notifier {
    pub fn new(me: String) -> Option<Notifier> {
        let helper = find_notifier()?;
        if !config::notify_enabled() {
            return None;
        }
        let (tx, rx) = sync_channel::<Msg>(512);
        std::thread::spawn(move || {
            let mut pending: Vec<Msg> = Vec::new();
            let mut warned = false;
            loop {
                let wait = if pending.is_empty() {
                    Duration::from_secs(3600)
                } else {
                    QUIET
                };
                match rx.recv_timeout(wait) {
                    Ok(m) => {
                        // Your own words do not need announcing back to you —
                        // but your own AI's do. A name belongs to a machine, so
                        // without the second test this would also silence the
                        // assistant sitting next to you.
                        if m.via != crate::msg::ACTOR_AI
                            && m.from.trim().eq_ignore_ascii_case(me.trim())
                        {
                            continue;
                        }
                        pending.push(m);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        flush(&helper, &pending, &mut warned);
                        pending.clear();
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Some(Notifier { tx })
    }

    pub fn send(&self, m: &Msg) {
        let _ = self.tx.try_send(m.clone()); // never block the watcher for a popup
    }
}

fn raise(
    helper: &PathBuf,
    title: &str,
    subtitle: &str,
    body: &str,
    channel: &str,
    warned: &mut bool,
) {
    // The last arguments are what a click needs: where the window is, what to
    // run to open it, and which channel the message came from.
    let self_path = std::env::current_exe().unwrap_or_default();
    let out = Command::new(helper)
        .args([
            title,
            body,
            subtitle,
            &gui_url(),
            &self_path.to_string_lossy(),
            channel,
            &config::name(),
        ])
        .output();
    match out {
        Ok(o) if !o.status.success() && !*warned => {
            *warned = true;
            eprintln!(
                "* notifications are not working ({}) {}",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) if !*warned => {
            *warned = true;
            eprintln!("* notifications are not working ({e})");
        }
        _ => {}
    }
}

fn flush(helper: &PathBuf, ms: &[Msg], warned: &mut bool) {
    match ms.len() {
        0 => {}
        1 => {
            let m = &ms[0];
            if m.is_change() {
                raise(
                    helper,
                    &m.who(),
                    &format!("{} · {}", m.action, m.target),
                    &ellipsis(&m.text, 180),
                    &m.channel,
                    warned,
                );
            } else {
                raise(
                    helper,
                    &m.who(),
                    &format!("#{}", m.channel),
                    &ellipsis(&m.text, 180),
                    &m.channel,
                    warned,
                );
            }
        }
        n => {
            let mut senders: Vec<String> = Vec::new();
            let mut changes = 0;
            for m in ms {
                if !senders.contains(&m.who()) {
                    senders.push(m.who());
                }
                if m.is_change() {
                    changes += 1;
                }
            }
            let title = if senders.len() > 2 {
                "collab".to_string()
            } else {
                senders.join(" & ")
            };
            let last = &ms[ms.len() - 1];
            let sub = if changes > 0 {
                format!(
                    "{n} new on #{} · {changes} change{}",
                    last.channel,
                    if changes == 1 { "" } else { "s" }
                )
            } else {
                format!("{n} new on #{}", last.channel)
            };
            let body = ellipsis(&format!("{}: {}", last.who(), last.line()), 180);
            raise(helper, &title, &sub, &body, &last.channel, warned);
        }
    }
}

pub fn test_notify() {
    let Some(h) = find_notifier() else {
        eprintln!("collab: no notifier installed for this platform");
        if cfg!(target_os = "macos") {
            eprintln!("        put Collab.app next to the collab binary, or in ~/Applications");
        } else {
            eprintln!("        put collab-notify.exe next to collab.exe");
        }
        std::process::exit(1);
    };
    println!("using {}", h.display());
    let mut warned = false;
    raise(
        &h,
        "collab",
        &format!("#{} · test", config::channel()),
        "If you can see this, notifications work.",
        &config::channel(),
        &mut warned,
    );
    if !warned {
        println!("sent — it should be on screen now (and in Notification Centre)");
    }
}
