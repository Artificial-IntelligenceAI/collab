# collab

A message channel between two machines on the same Wi-Fi, so two Claude sessions
can tell each other what they just did.

    collab serve              run the server (one machine only)
    collab watch              stream messages — this is what Monitor runs
    collab post "message"     send a chat message
    collab change ...         record a structured change
    collab log [-changes]     history
    collab who                what name, channel, server and key are in force
    collab key [-new]         show or create the shared key
    collab test-notify        check that popup notifications work
    collab mcp                run as an MCP server (tools only)

On the Mac there is also **Collab.app** — a menu bar app that is both the window
and the notifications.

Env: `COLLAB_HOST`, `COLLAB_PORT` (8787), `COLLAB_NAME`, `COLLAB_CHANNEL`,
`COLLAB_KEY`, `COLLAB_NOTIFY` (`0` turns popups off)

## Everything is encrypted

One shared word, the same on both machines:

    collab key -new     # prints a five-word key and saves it here
                        # copy the line it prints to the other machine

Until both sides match they cannot talk at all, which is the failure you want.

That word becomes a real key through **Argon2id**, and every frame is sealed with
**XChaCha20-Poly1305**, which authenticates as well as encrypts — a tampered frame
does not open at all rather than opening into something subtly wrong. The server
states a fresh random challenge when you connect, and that challenge is the
associated data for every frame after it, so a frame captured from an earlier
connection cannot be replayed into a later one.

This is not only about privacy. Before it, `nc` against the port printed the whole
history to anyone on the Wi-Fi, and the server took the client's word for who it
was — so anything on the network could forge a change record. This tool exists so
neither AI has to guess what the other did, and an unauthenticated wire let exactly
that guess arrive from outside.

The history file and the config that holds the key are both `chmod 600`. Encrypting
the history while the key sat world-readable beside it would have been theatre.

## Who you are

A name is `COLLAB_NAME`, or your computer's hostname. No accounts, nothing to sign
up for. Settings come from the environment first, then `~/.collab-config`, then the
default:

    name    = tankun
    channel = roblox
    key     = five-words-from-collab-key

The file is not just a convenience. A collab started by the MCP server, by launchd,
or by clicking a notification inherits none of your shell, so anything set only in
`.zshrc` works in a terminal and quietly fails everywhere else — posting under your
hostname, on the wrong channel, with no key. `collab who` says what is actually in
force and where each answer came from.

**A name belongs to a machine, not a person** — which means it covers you *and* your
Claude. So every message records whether a person or an AI sent it, and shows as
`sis` or `sis's AI`. The MCP tools are the AI; the window's text box is you; a
command typed in a terminal is taken to be you.

## Two kinds of message

**chat** is free text. **change** is structured: who, which script or instance, what
kind of change, and a one-line summary.

    collab change -action edited -target "ServerScriptService/ShopHandler" \
        "gave the buy button a debounce so double-clicks stop double-charging"

`-action` is one of `added`, `edited`, `removed`, `renamed`, and nothing else is
accepted. Both kinds share the sequence numbering and the same channel, so nothing
is lost either way — the Changes view just filters to the second kind.

Recording a change is a **deliberate act**, never something parsed out of prose. A
change record inferred from someone's sentence is a guess wearing a fact's clothes,
and this whole tool exists so neither AI has to guess what the other did.

## The app

`Collab.app` lives in the menu bar rather than the Dock, because notifications
should arrive whether or not a window is open, and a window you have to keep open
is a window you will close.

- **Chat** — everything, in order, with day separators and a box to type in.
- **Changes** — *a git log for a project that can't use git.* Roblox saves a binary
  `.rbxl`, so git has nothing to show. But the AIs know what they changed, because
  they made the changes. Consecutive changes by one person within 15 minutes are
  grouped into one entry, the way `git log` groups edits into a commit.
- **Search** and a **channel picker** apply to both. You post where you are looking:
  switch the picker and the composer follows, because reading one channel and
  typing into another would be a nasty little trap.
- Solarized, light and dark, following the OS.

Notifications are the app's, not the command line's — macOS attributes a
notification to a bundle's main executable, so the CLI cannot raise one and a second
binary inside the same bundle is refused outright. `collab test-notify` asks the app
through a `collab://` URL. **A burst is one popup, not forty**: a machine waking from
sleep is replayed everything it missed at once, so arrivals are collected until the
channel goes quiet for 700 ms and a burst becomes a summary. Your own typing never
pops; **your own AI does**, because you are usually looking at Roblox Studio and not
at the session.

The app never speaks the protocol itself. It runs `collab watch -json` and reads
what comes back, so there is one implementation of the wire format and one of the
encryption. Swift holding a second copy of the crypto would be a second thing to get
wrong.

## Why it works the way it does

**Every message has a sequence number, and a watcher remembers the last one it saw.**
On reconnect it asks the server to resume from there, so a message sent while you
were offline still arrives — exactly once. A dropped message and a quiet channel
must never look the same. If a watcher cannot even write down its place, it says so
rather than silently replaying history later.

**Disconnection is announced.** `* DISCONNECTED — retrying`, then `* reconnected,
resuming from #N`, and the same thing in the app as a red banner. Silence should mean
nobody is talking, never that the wire died. Nothing that failed to send is ever
reported as sent — the far side must prove it holds the key before a message is
called delivered, or a message nobody could read would look exactly like one that
arrived.

**The server survives restarts** — it reads the history file and resumes numbering,
and it owns the only complete copy, so the other machine asks over the wire.

**MCP is tools only, no resources, no subscriptions.** Tested on 2026-08-19: a server
pushed 25 notifications over 8 minutes, via both `notifications/resources/updated`
and `notifications/message`, and Claude Desktop never subscribed and never reacted.
It is a pull-only client. So notifications come from `Monitor` running `collab watch`,
and MCP only makes posting and recording typed tool calls.

## Build and install

    ./build.sh      # everything, into dist/
    ./install.sh    # this Mac's half; also how you upgrade

`build.sh` needs Rust and Xcode's Swift for the Mac half. The Windows half needs the
`x86_64-pc-windows-gnu` Rust target and the .NET SDK, and is skipped with a warning
rather than shipping a half-built folder.

Do not install by copying over the old binary yourself. Writing over a signed binary
in place leaves macOS holding a stale code signature, and the kernel then kills it on
sight **with no error message at all** — the command simply dies. `install.sh` deletes
before it copies.

Two LaunchAgents are installed, and both matter: one runs the server, the other
starts the app at login. A machine where only the server came back is a machine
that receives messages silently, because the app is what raises the popups.

Then `collab key -new` if you have not already, and `collab test-notify` once. macOS
asks whether to allow notifications from "collab" the first time; say yes.

**MCP** — register the core once per machine, in `~/.claude.json` on the Mac or
`%USERPROFILE%\.claude.json` on Windows:

    "mcpServers": { "collab": { "command": "/Users/you/.local/bin/collab", "args": ["mcp"] } }

An absolute path on purpose: the server is spawned by the app, which does not
necessarily have `~/.local/bin` on its PATH. No `env` block is needed — name,
channel and key all come from `~/.collab-config`, and the core looks at `HOME` and
then `USERPROFILE`, so the same arrangement works on both machines.

## Windows

Not finished. The Rust core cross-compiles once the target is installed:

    rustup target add x86_64-pc-windows-gnu && brew install mingw-w64

`notify/windows` is the C# toast helper, built but never run — there is no Windows
machine here. A native window for that side is still to do.

## Files

    build.sh    builds both machines' worth of it into dist/
    install.sh  installs this Mac's half, and upgrades it
    com.tankun.collab.plist      LaunchAgent for the server
    com.tankun.collab.app.plist  LaunchAgent for the app

    core/src/main.rs     command dispatch
    core/src/config.rs   settings, ~/.collab-config, `collab who`
    core/src/crypto.rs   key derivation and frame sealing
    core/src/wire.rs     the connection: a challenge, then nothing in the clear
    core/src/server.rs   the hub: sequence numbers, subscribers, replay-on-connect
    core/src/client.rs   watch, post, change, log, and the reconnect rule
    core/src/history.rs  the file the server owns
    core/src/notify.rs   finding the notifier, and coalescing bursts into one popup
    core/src/mcp.rs      the MCP tools
    core/src/msg.rs      what travels on the wire

    app/mac/Sources/     the menu bar app and its window
    app/mac/icon.swift   generates the icon both platforms use
    notify/windows/      C# toast helper (unverified)
