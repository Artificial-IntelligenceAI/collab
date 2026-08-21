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
#[cfg(not(target_os = "macos"))]
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

/// A machine waking from sleep is replayed everything it missed at once, and
/// forty popups in a row is not a notification, it is a punishment. Arrivals
/// are collected until the channel goes quiet.
#[cfg(not(target_os = "macos"))]
const QUIET: Duration = Duration::from_millis(700);

/// On macOS this is Collab.app, which raises notifications itself: macOS
/// attributes a notification to a bundle's main executable, so a second binary
/// inside the same bundle is refused outright — and this binary has no bundle
/// at all. On Windows it is the standalone toast helper.
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
            candidates.push(base.join("Collab.app"));
        }
    } else if cfg!(target_os = "windows") {
        candidates.push(dir.join("collab-notify.exe"));
        candidates.push(dir.join("notify/collab-notify.exe"));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Hands a request to the app — the only thing here allowed to raise one.
/// `-g` keeps it from stealing focus.
#[cfg(target_os = "macos")]
fn ask_app(url: &str) -> std::io::Result<()> {
    match Command::new("open").args(["-g", url]).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(std::io::Error::other("Collab.app did not answer")),
        Err(e) => Err(e),
    }
}

#[cfg(target_os = "macos")]
pub fn ensure_app_running() -> bool {
    let Some(app) = find_notifier() else { return false };
    Command::new("open")
        .args(["-g", "-a"])
        .arg(&app)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn gui_url() -> String {
    format!(
        "http://127.0.0.1:{}",
        config::env("COLLAB_GUI_PORT", "8788")
    )
}

#[cfg(not(target_os = "macos"))]
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

        // On macOS the app watches the channel itself and posts its own
        // notifications, so the right thing here is to make sure it is running
        // and then stay out of the way — two notifiers would double every popup.
        #[cfg(target_os = "macos")]
        {
            let _ = (me, helper);
            ensure_app_running();
            None
        }

        #[cfg(not(target_os = "macos"))]
        {
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
                        // Addressed to somebody else: still delivered, not announced.
                        if !m.is_for(&config::my_names()) {
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
    }

    pub fn send(&self, m: &Msg) {
        let _ = self.tx.try_send(m.clone()); // never block the watcher for a popup
    }
}

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
fn flush(helper: &PathBuf, ms: &[Msg], warned: &mut bool) {
    match ms.len() {
        0 => {}
        1 => {
            let m = &ms[0];
            // The machine rides along in the title: a chat that named itself
            // "shop" says nothing about whose Claude it is, and that is the
            // question the popup has to answer.
            if m.is_change() {
                raise(
                    helper,
                    &m.label(),
                    &format!("{} · {}", m.action, m.target),
                    &ellipsis(&m.text, 180),
                    &m.channel,
                    warned,
                );
            } else {
                raise(
                    helper,
                    &m.label(),
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
                if !senders.contains(&m.label()) {
                    senders.push(m.label());
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
            let body = ellipsis(&format!("{}: {}", last.label(), last.line()), 180);
            raise(helper, &title, &sub, &body, &last.channel, warned);
        }
    }
}

/// macOS: the app is what posts notifications, so ask it.
#[cfg(target_os = "macos")]
pub fn test_notify() {
    let Some(app) = find_notifier() else {
        eprintln!("collab: Collab.app is not installed — run ./install.sh");
        std::process::exit(1);
    };
    println!("asking {} to post one", app.display());
    if !ensure_app_running() {
        eprintln!("collab: could not start Collab.app");
        std::process::exit(1);
    }
    std::thread::sleep(Duration::from_millis(700)); // let it finish launching
    match ask_app("collab://test") {
        Ok(()) => println!("sent — it should be on screen now (and in Notification Centre)"),
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn test_notify() {
    let Some(h) = find_notifier() else {
        eprintln!("collab: no notifier installed — put collab-notify.exe next to collab.exe");
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
        println!("sent — it should be on screen now (and in the Action Centre)");
    }
}
