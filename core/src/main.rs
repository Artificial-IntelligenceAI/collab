//! collab — a message channel between two machines, so two AI sessions can tell
//! each other what they just did.
//!
//! Everything on the wire is encrypted with a word the two machines share, and
//! a message that will not decrypt is not a message. That is not only about
//! privacy: this tool exists so neither AI has to guess what the other did, and
//! an unauthenticated wire would let anything on the network forge a change
//! record — a guess wearing a fact's clothes, arriving from outside.
mod channels;
mod client;
mod config;
mod crypto;
mod files;
mod history;
mod mcp;
mod msg;
mod notify;
mod release;
mod server;
mod setup;
mod wire;

const USAGE: &str = "usage:
  collab serve                          run the server (one machine only)
  collab watch [-json] [-notify] [-all] [-since N] [-no-save]
                                        stream messages — this is what Monitor runs
  collab post \"message\" [-ai] [-c chan] send a chat message
  collab change -action edited -target \"ServerScriptService/Shop\" \"what changed\" [-c chan]
  collab log [-changes] [-all]          history
  collab setup                          first-run setup: server or client, then prove it
  collab who                            show the name, channel, server and key in use
  collab channels [-keys]               channels on this machine
  collab channel add <invite>           join with the invite someone sent you
  collab channel delete <name>          close it everywhere (only where it was made)
  collab channel forget <name>          leave it (drops your key only)
  collab update [-yes]                  check for a signed update, and install it
  collab test-notify                    check that popup notifications work
  collab mcp                            run as an MCP server";

/// Pulls `-name value` out of the arguments, leaving the rest as free text.
/// A message must not be able to swallow a flag. Someone reaching for one that
/// does not exist is usually testing exactly the thing it would have changed —
/// `post -c other "..."` before -c existed went to the default channel, carried
/// "-c other" in its body, and reported success. Only the first word is checked,
/// so a message may still begin with a dash.
fn reject_unknown_flag(args: &[String], cmd: &str) {
    let Some(first) = args.first() else { return };
    let looks_like_flag = first.len() > 1
        && first.starts_with('-')
        && first[1..].starts_with(|c: char| c.is_ascii_alphabetic());
    if looks_like_flag {
        eprintln!(
            "collab: {cmd} does not know the option {first}, and will not send it as text.\n\n{}",
            USAGE
        );
        std::process::exit(2);
    }
}

fn take_flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    if i < args.len() {
        Some(args.remove(i))
    } else {
        None
    }
}

fn take_switch(args: &mut Vec<String>, name: &str) -> bool {
    match args.iter().position(|a| a == name) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let cmd = args.remove(0);

    match cmd.as_str() {
        "serve" => server::serve(),
        "watch" => {
            let as_json = take_switch(&mut args, "-json");
            let popups = take_switch(&mut args, "-notify");
            let all = take_switch(&mut args, "-all");
            let no_save = take_switch(&mut args, "-no-save");
            let since = take_flag(&mut args, "-since").and_then(|v| v.parse().ok());
            client::watch(as_json, popups, since, !no_save, all)
        }
        "post" => {
            let ai = take_switch(&mut args, "-ai");
            let channel = take_flag(&mut args, "-c");
            reject_unknown_flag(&args, "post");
            client::post(&args.join(" "), ai, channel.as_deref())
        }
        "change" => {
            let ai = take_switch(&mut args, "-ai");
            let channel = take_flag(&mut args, "-c");
            let action = take_flag(&mut args, "-action").unwrap_or_default();
            let target = take_flag(&mut args, "-target").unwrap_or_default();
            reject_unknown_flag(&args, "change");
            client::change(&action, &target, &args.join(" "), ai, channel.as_deref())
        }
        "log" => client::show_log(
            take_switch(&mut args, "-changes"),
            take_switch(&mut args, "-all"),
        ),
        "send" => {
            let channel = take_flag(&mut args, "-c");
            let caption = take_flag(&mut args, "-m").unwrap_or_default();
            client::send_file_cmd(
                args.first().map(String::as_str).unwrap_or(""),
                &caption,
                channel.as_deref(),
            )
        }
        "users" => {
            let all = take_switch(&mut args, "-all");
            client::users_cmd(take_flag(&mut args, "-c").as_deref(), all)
        }
        "files" => client::files_cmd(take_flag(&mut args, "-c").as_deref()),
        "get" => {
            let channel = take_flag(&mut args, "-c");
            let out = take_flag(&mut args, "-o");
            client::get_file_cmd(
                args.first().map(String::as_str).unwrap_or(""),
                out.as_deref(),
                channel.as_deref(),
            )
        }
        "update" => release::update_cmd(
            take_switch(&mut args, "-yes"),
            take_switch(&mut args, "-json"),
        ),
        "release" => {
            match args.first().map(String::as_str) {
                Some("keygen") => release::keygen(),
                Some("sign") => {
                    let version = take_flag(&mut args, "-version").unwrap_or_default();
                    let notes = take_flag(&mut args, "-notes").unwrap_or_default();
                    let key = take_flag(&mut args, "-key").unwrap_or_default();
                    let dir = args.get(1).map(String::as_str).unwrap_or("");
                    if dir.is_empty() || version.is_empty() || key.is_empty() {
                        eprintln!("usage: collab release sign <dir> -version X.Y.Z -key <private> [-notes \"...\"]");
                        std::process::exit(2);
                    }
                    if let Err(e) =
                        release::sign_release(std::path::Path::new(dir), &version, &notes, &key)
                    {
                        eprintln!("collab: {e}");
                        std::process::exit(1);
                    }
                }
                Some("verify") => {
                    let dir = args.get(1).map(String::as_str).unwrap_or("");
                    match release::verify_dir(std::path::Path::new(dir)) {
                        Ok(man) => {
                            println!("verified: version {}, {} file(s), signed by the key this build trusts",
                                 man.version, man.files.len());
                        }
                        Err(e) => {
                            eprintln!("collab: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                _ => {
                    eprintln!("usage: collab release keygen");
                    eprintln!("       collab release verify <dir>");
                    eprintln!("       collab release sign <dir> -version X.Y.Z -key <private>");
                    std::process::exit(2);
                }
            }
        }
        "setup" => setup::run(),
        "who" => {
            if take_switch(&mut args, "-json") {
                config::who_json()
            } else {
                config::who()
            }
        }
        "channels" => {
            let json = take_switch(&mut args, "-json");
            client::channels_cmd(take_switch(&mut args, "-keys"), json)
        }
        "channel" => match args.first().map(String::as_str) {
            Some("create") => client::channel_create(args.get(1).map(String::as_str).unwrap_or("")),
            Some("add") => client::channel_add(
                args.get(1).map(String::as_str).unwrap_or(""),
                args.get(2).map(String::as_str).unwrap_or(""),
            ),
            Some("delete") => client::channel_delete(args.get(1).map(String::as_str).unwrap_or("")),
            Some("forget") => client::channel_forget(args.get(1).map(String::as_str).unwrap_or("")),
            _ => {
                eprintln!("usage: collab channel add <name> <key>");
                eprintln!("       collab channel delete <name>   (only where it was made)");
                eprintln!("       collab channel forget <name>   (leave; drops your key only)");
                eprintln!("       (new channels are made in the collab app, by a person)");
                std::process::exit(2);
            }
        },
        "test-notify" => notify::test_notify(),
        "mcp" => mcp::run(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
