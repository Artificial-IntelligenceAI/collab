//! collab — a message channel between two machines, so two AI sessions can tell
//! each other what they just did.
//!
//! Everything on the wire is encrypted with a word the two machines share, and
//! a message that will not decrypt is not a message. That is not only about
//! privacy: this tool exists so neither AI has to guess what the other did, and
//! an unauthenticated wire would let anything on the network forge a change
//! record — a guess wearing a fact's clothes, arriving from outside.
mod client;
mod config;
mod crypto;
mod history;
mod mcp;
mod msg;
mod notify;
mod server;
mod wire;

const USAGE: &str = "usage:
  collab serve                          run the server (one machine only)
  collab watch [-json] [-notify] [-all] [-since N] [-no-save]
                                        stream messages — this is what Monitor runs
  collab post \"message\" [-ai]           send a chat message
  collab change -action edited -target \"ServerScriptService/Shop\" \"what changed\"
  collab log [-changes] [-all]          history
  collab who                            show the name, channel, server and key in use
  collab key [-new]                     show or create the shared key
  collab test-notify                    check that popup notifications work
  collab mcp                            run as an MCP server";

/// Pulls `-name value` out of the arguments, leaving the rest as free text.
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
            client::post(&args.join(" "), ai)
        }
        "change" => {
            let ai = take_switch(&mut args, "-ai");
            let action = take_flag(&mut args, "-action").unwrap_or_default();
            let target = take_flag(&mut args, "-target").unwrap_or_default();
            client::change(&action, &target, &args.join(" "), ai)
        }
        "log" => client::show_log(
            take_switch(&mut args, "-changes"),
            take_switch(&mut args, "-all"),
        ),
        "who" => {
            if take_switch(&mut args, "-json") {
                config::who_json()
            } else {
                config::who()
            }
        }
        "key" => client::key_cmd(take_switch(&mut args, "-new")),
        "test-notify" => notify::test_notify(),
        "mcp" => mcp::run(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
