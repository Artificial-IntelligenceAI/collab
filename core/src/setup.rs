//! First-run setup: which machine this is, and proving it works before saying so.
//!
//! The whole thing turns on one question — is this the server, or does it talk
//! to one — because every other answer follows from it. The two roles are not a
//! checkbox on one install: the server keeps a port open, starts at login and
//! accumulates every channel key and all the history; a client needs none of
//! that and needs an address instead.
//!
//! Nothing is reported as done until it has been shown to work. A wrong address
//! does not announce itself: it simply never connects, and from the far end that
//! is indistinguishable from nobody having said anything yet.
use crate::channels;
use crate::config;
use crate::wire::PROTOCOL;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

fn ask(question: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{question}: ");
    } else {
        print!("{question} [{default}]: ");
    }
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    let s = s.trim().to_string();
    if s.is_empty() {
        default.to_string()
    } else {
        s
    }
}

fn ask_secret(question: &str) -> String {
    print!("{question}: ");
    let _ = std::io::stdout().flush();
    let hide = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if hide {
        let _ = std::process::Command::new("stty").arg("-echo").status();
    }
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    if hide {
        let _ = std::process::Command::new("stty").arg("echo").status();
        println!();
    }
    s.trim().to_string()
}

fn yes(question: &str) -> bool {
    matches!(
        ask(&format!("{question} (y/n)"), "y")
            .to_lowercase()
            .as_str(),
        "y" | "yes"
    )
}

/// Is there a collab server there, and does it speak our protocol? The greeting
/// is sent before any key, so this can be answered without one — which is what
/// lets an address be checked before a key has even been handed over.
pub fn probe(addr: &str) -> Result<u32, String> {
    let target = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:{}", config::port())
    };
    let sock: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&target)
        .map_err(|_| {
            format!("cannot look up {target} — check the spelling, or use the IP address")
        })?
        .collect();
    let first = sock
        .first()
        .ok_or_else(|| format!("{target} resolves to nothing"))?;
    let stream = TcpStream::connect_timeout(first, Duration::from_secs(5))
        .map_err(|e| format!("nothing answered at {target} — {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|_| format!("{target} accepted the connection but said nothing"))?;
    let v: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|_| format!("something is listening on {target}, but it is not collab"))?;
    v.get("collab")
        .and_then(|c| c.as_u64())
        .map(|c| c as u32)
        .ok_or_else(|| format!("something is listening on {target}, but it is not collab"))
}

fn save(pairs: &[(&str, String)]) -> std::io::Result<()> {
    let path = config::config_path();
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        "# Who you are and where you talk, for every way collab gets started.\n\
         # The environment still wins over anything here.\n\n"
            .to_string()
    });
    for (k, v) in pairs {
        let kept: Vec<&str> = text
            .lines()
            .filter(|l| {
                !l.split_once('=')
                    .map(|(a, _)| a.trim().eq_ignore_ascii_case(k))
                    .unwrap_or(false)
            })
            .collect();
        text = kept.join("\n");
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("{k} = {v}\n"));
    }
    std::fs::write(&path, text)?;
    config::lock_down(&path);
    Ok(())
}

pub fn run() {
    println!("collab setup\n");
    println!("One machine runs the server. Everyone else connects to it. The server needs to");
    println!(
        "be awake for anybody to send anything, so it should be the machine that stays put.\n"
    );

    let server = yes("Is this machine the server?");
    println!();

    if server {
        setup_server();
    } else {
        setup_client();
    }
}

fn setup_server() {
    let name = ask(
        "What should you be called on the channel?",
        &config::hostname(),
    );
    let _ = save(&[("name", name.clone())]);

    // A server with no channel is a server nobody can connect to, since a
    // connection is opened by a channel's key.
    let existing = channels::names();
    let channel = if existing.is_empty() {
        let c = ask("Name a channel to start with", "general");
        match channels::create(&c) {
            Ok((n, _)) => n,
            Err(e) => {
                eprintln!("collab: {e}");
                std::process::exit(1);
            }
        }
    } else {
        println!("channels already here: {}", existing.join(", "));
        existing[0].clone()
    };
    let _ = save(&[("channel", channel.clone())]);

    // Asked rather than assumed: someone may already run it another way, and a
    // setup that silently replaces a working service is a setup that breaks one.
    #[cfg(target_os = "macos")]
    if yes("Start the server automatically at login?") {
        install_agent();
    } else {
        println!("  leaving that alone — start it yourself with:  collab serve");
    }

    println!();
    match probe("localhost") {
        Ok(v) => println!(
            "✓ server answering on port {} (collab v{v})",
            config::port()
        ),
        Err(e) => {
            println!("✗ the server is not answering yet — {e}");
            println!("  start it with:  collab serve");
        }
    }

    let key = channels::get(&channel).map(|c| c.key).unwrap_or_default();
    println!("\n─────────────────────────────────────────────");
    println!("Give the other machine these three things:\n");
    println!("  address   {}", config::hostname());
    println!("  channel   {channel}");
    println!("  key       {key}");
    println!("\nIf the address does not work over there, use this machine's IP instead.");
    println!("─────────────────────────────────────────────");
}

fn setup_client() {
    let name = ask(
        "What should you be called on the channel?",
        &config::hostname(),
    );

    // Checked before anything is written: an address that does not resolve is
    // the failure that costs an afternoon, because it looks like silence.
    let host = loop {
        let h = ask("Address of the machine running the server", "");
        if h.is_empty() {
            continue;
        }
        match probe(&h) {
            Ok(v) if v == PROTOCOL => {
                println!("✓ reached it (collab v{v})");
                break h;
            }
            Ok(v) => {
                println!("✗ that machine runs collab v{v} and this one is v{PROTOCOL} — update the older one");
            }
            Err(e) => {
                println!("✗ {e}");
                println!("  a name ending in .local often does not resolve; try the IP address");
            }
        }
        if !yes("Try a different address?") {
            std::process::exit(1);
        }
    };

    let channel = ask("Channel name (the other machine printed it)", "general");
    let key = ask_secret("Channel key (it will not be shown)");
    if let Err(e) = channels::add(&channel, &key, "") {
        eprintln!("collab: {e}");
        std::process::exit(1);
    }
    let channel = channels::tidy(&channel);
    let _ = save(&[("name", name), ("host", host), ("channel", channel.clone())]);

    // The address proved there is a server; this proves the key opens it.
    println!();
    let seen = crate::client::fetch(&channel, 0);
    if seen.is_empty() {
        println!("✓ joined #{channel} — nothing there yet");
    } else {
        println!(
            "✓ joined #{channel} — {} message(s) already there",
            seen.len()
        );
        if let Some(last) = seen.last() {
            println!("  latest: {}: {}", last.label(), last.line());
        }
    }
    println!("\nDone. Open Collab to see it.");
}

#[cfg(target_os = "macos")]
fn install_agent() {
    let label = "com.tankun.collab";
    let home = config::home("").display().to_string();
    let home = home.trim_end_matches('/').to_string();
    let exe = std::env::current_exe()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "collab".into());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>              <string>{label}</string>
  <key>ProgramArguments</key>   <array>
                                  <string>{exe}</string>
                                  <string>serve</string>
                                </array>
  <key>RunAtLoad</key>          <true/>
  <key>KeepAlive</key>          <true/>
  <key>StandardOutPath</key>    <string>{home}/.collab-server.log</string>
  <key>StandardErrorPath</key>  <string>{home}/.collab-server.log</string>
</dict>
</plist>
"#
    );
    let dir = config::home("Library/LaunchAgents");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{label}.plist"));
    if std::fs::write(&path, plist).is_err() {
        println!("✗ could not write {}", path.display());
        return;
    }
    let uid = libc_getuid();
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{label}")])
        .status();
    let ok = std::process::Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{uid}")])
        .arg(&path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!(
        "{} server set to start at login",
        if ok { "✓" } else { "✗" }
    );
}

#[cfg(target_os = "macos")]
fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}
