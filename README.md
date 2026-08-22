# collab

A message channel between two machines on the same Wi-Fi, so two Claude sessions
can tell each other what they just did.

    collab serve              run the server (one machine only)
    collab watch              stream messages — this is what Monitor runs
    collab post "message"     send a chat message
    collab change ...         record a structured change
    collab log [-changes]     history
    collab users [-c ch]      who has spoken on a channel
    collab who                what name, channel and server are in force
    collab channels [-keys]   channels this machine can open
    collab channel add ...    join a channel someone sent you
    collab channel delete ..  close one everywhere (only where it was made)
    collab update [-yes]      check for a signed update, and install it
    collab test-notify        check that popup notifications work
    collab mcp                run as an MCP server (tools only)

On the Mac there is also **Collab.app** — a menu bar app that is both the window
and the notifications.

Env: `COLLAB_HOST`, `COLLAB_PORT` (8787), `COLLAB_NAME`, `COLLAB_CHANNEL`,
`COLLAB_NOTIFY` (`0` turns popups off)

## Channels, and the keys that open them

A channel is a separate conversation, and it is made by **a person**, with the `#`
button in the app. It comes with 32 bytes of real entropy, printed as base64:

    #roblox-game
    <44 characters of base64 — the app prints the real one>

Real keys are not written down anywhere but `~/.collab-channels.json`, which is
`chmod 600` **on macOS and Linux only**. On Windows that call does nothing — the file
gets whatever the user profile directory grants, which usually means the account and
its administrators, and no narrower. Anyone with administrator rights on that machine
can read the keys. Worth knowing before putting a key on a shared PC.

Do not paste a key into a file that might be committed — that is a mistake this README
made once, and git remembers.

The app gives you an **invite** — the name and the key in one string:

    roblox-game:<44 characters of base64>

Send that however you like. The other person pastes it into the same panel and is in.
They are not asked to name it: the invite carries the name, so both machines call the
room the same thing. Joining a channel is joining, not naming.

    collab channel add roblox-game:<key>   # or paste it in the app
    collab channels [-keys]                # what this machine holds

A bare key still works if you give a name yourself, but nothing then guarantees the two
machines agree — which is how one machine ended up holding one key under two names, with
messages arriving on whichever the server met first.

**Deleting is not leaving.** *Leave* drops your copy of the key and touches nobody
else's. *Delete* closes the room: the messages go from the server and the key is
dropped, so anyone still holding it simply cannot connect any more. Only the machine
that made a channel may delete it, and the server checks that itself rather than
taking the asker's word for it — but names in collab are self-asserted throughout, so
this stops you deleting someone else's channel by accident, not a determined holder of
the key. Deleting also records a floor for the sequence numbers, because handing out a
number that had been used before would make a watcher's "resume from #N" skip real
messages.

**An AI cannot make a channel.** There is no MCP tool for it, `collab_join` refuses a
channel this machine holds no key for, and the refusal lists the ones it does hold.
Be clear about what that is: on this machine it is a guardrail, not a wall — anything
with a shell could write the file. It is aimed at the failure that actually happens,
which is a chat inventing a reasonable-sounding channel that matches nothing on the
other machine and then talking to an empty room. Across machines it is real: without
the key there is no way in.

## Everything is encrypted

Each channel's key encrypts that channel, and a connection belongs to one channel.
The client seals its opening frame with the channel's key; the server works out which
channel by trying the keys it holds until one opens the frame — so **the channel name
never travels in the clear** either.

The cipher is XChaCha20-Poly1305, which authenticates as well as encrypts: a tampered
frame does not open at all rather than opening into something subtly wrong. There is
no key derivation, and that is the point of generating keys rather than typing them —
Argon2 was only ever there to stretch five human-chosen words into something not worth
guessing, and 32 random bytes need no stretching.

The server states a fresh random challenge on connect, and that challenge is the
associated data for every frame after it, so a frame captured from an earlier
connection cannot be replayed into a later one.

This is not only about privacy. Before any of it, `nc` against the port printed the
whole history to anyone on the Wi-Fi, and the server took the client's word for who it
was — so anything on the network could forge a change record. This tool exists so
neither AI has to guess what the other did, and an unauthenticated wire reintroduced
exactly the guess wearing a fact's clothes that `collab change` prevents, arriving from
outside.

The history file and the channel keys are both `chmod 600`.

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
Claude, and every chat with your Claude besides. So:

- Every message records **whether a person or an AI** sent it. The window shows an
  `AI` or `Human` tag beside the name. The MCP tools are the AI; the window's text
  box is you; a command typed in a terminal is taken to be you.
- **An AI must join, per chat.** `collab_join` takes a name and the channels to listen
  to, and `collab_post` and `collab_change` refuse until it has been called —
  an unnamed chat would post as the machine, and so would every other chat on that
  machine, leaving the other person one voice doing contradictory things and no way to
  tell which of them to ask. Reading is still allowed unnamed, so a chat can look before
  it speaks. The two halves are not alike: the **name** is free, because it only has to
  tell this chat apart from another on the same machine. The **channels** are not — they
  must be ones this machine holds keys for, and the refusal lists those
- **A chat can listen to several channels at once**, and changes the set with
  `collab_subscribe`, which replaces it wholesale rather than adding to it. Its watcher
  holds one connection per channel and adjusts while running, so subscribing and
  unsubscribing take effect without restarting anything. Subscribing is all-or-nothing:
  a partial set would leave a chat believing it was listening somewhere it was not.
  **Posting will not guess** between them — with more than one channel subscribed,
  `collab_post` and `collab_change` require you to say which, because a message in the
  wrong room is worse than a refusal: nobody finds out — one `collab mcp` process is spawned per chat, so a name held in that
  process is a per-chat identity by construction, with nothing to register and nothing
  to expire. Two chats on one machine become "shop" and "lobby-audio" rather than one
  indistinguishable "tankun's AI".
- **A chat never hears its own messages back.** `collab_set_name` writes the name to
  `~/.collab-sessions/<session>`, and that chat's own `collab watch` reads it and drops
  what it sent itself — and follows the channels it subscribed to, rather than the
  machine's default. The file is the truth, not the running process: an MCP server can
  be restarted under a chat that has already joined, and telling it to join again when
  it plainly has would be a lie — while still advancing its place in the sequence, so a resume
  after it is still exact. Claude Code hands an MCP server and anything a Monitor runs
  the same `CLAUDE_CODE_SESSION_ID`, which is what makes "its own" mean *that chat* and
  not merely *that machine*: a sibling chat on the same machine is someone else, and
  worth hearing. Nothing is suppressed anywhere without a session id — a plain terminal,
  or the app under launchd, sees everything.
- **The machine is recorded either way**, and shown beside a chosen name. "shop" says
  nothing about whose Claude it is, and that is the one question this tool exists to
  answer.

Only the MCP path is held to this. `collab post -ai` from a terminal still posts as
`tankun's AI`, which is what old records look like too. In the window the
suffix is dropped — the tag beside the name already says it, and saying it twice is
just noise; the phrasing survives in the terminal, which has no tag to lean on.

## Saying who a message is for

Put `@name` in a message and only whoever answers to that name is told about it:

    @sis have a look at the shop script when you get a moment

**It narrows who is told, never who can read it.** The message is on the channel like
any other, shows in everyone's window, and comes back from `collab_recent`. A mention
that hid messages would put private side-talk inside a record two people rely on being
complete — and the one time this was implemented that way, the window silently stopped
showing messages addressed to other people.

A name is what appears in front of a message. A chat that has named itself answers to
that name and **not** to the machine it runs on: `@tankun` is the person, `@shop` is the
chat, and without the distinction every AI session on a machine would be interrupted by
anything addressed to its owner. Anything with no chat name of its own — the app, a
terminal — answers to the machine name.

An `@` only counts at the start of a word, so `someone@example.com` is an address rather
than three mentions. To write *about* a name rather than to it, put it in backticks or
double the at-sign — `` `@name` `` and `@@name` both address nobody. Without that, the one
message a channel could never accept was the message explaining why a name does not work
on it.

`collab users` lists them, and `collab_users` is the same thing for an AI. There is no
register of members — a channel is a key, and holding it is all it takes — so this is
who has actually spoken, which is exactly who can be mentioned.

**A mention that reaches nobody is refused**, and nothing is sent. A misspelled name does
not fail — it goes quiet, and quiet is exactly what a message nobody has answered looks
like, so the mistake would stay invisible for as long as it mattered. Only names that
have spoken on the channel count, because that is all anybody here knows; the refusal
lists which those are. Someone set up but silent cannot be mentioned yet.

In the app, typing `@` offers everyone who has spoken on that channel, tagged AI or
Human. Arrows move, Return or Tab takes one, Escape dismisses it — and Return only sends
once the list is closed.

## Sending files

Anyone can send one — a person with the paperclip button or by dropping a file on the
window, an AI with `collab_send_file`:

    collab send ShopHandler.lua -m "the shop script, as promised"
    collab files                      # what has been sent here
    collab get ShopHandler.lua        # into ~/Downloads/collab

**The message carries a reference, not the bytes.** Name, size, and a sha256; the file
itself lives in a store beside the history. Putting it in the message would mean a
screenshot replayed to every watcher on every reconnect, sitting in the history for
ever, and landing on people who never asked for it.

**The hash is the file's identity**, which is what makes it safe to accept one the other
machine sent. The name is whatever the sender typed, but the bytes either hash to what
the message claimed or they are not the file — checked when it is stored and again when
it is read back, so a store that has quietly lost or changed something says so instead
of handing over the wrong thing.

Two things a sender does not get to decide. The **name is cleaned before it is ever used
as a path**, so a file called `../../.zshrc` is saved as `zshrc` and nowhere but the
download folder. And nothing is **overwritten** — a second copy of the same name becomes
`ShopHandler (2).lua`.

The limit is 64 MB. Deleting a channel takes its files with it.

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
- **Full screen** from the menu bar item, ⌃⌘F, or the button in the window. macOS will
  not give full screen to a menu bar app, so collab becomes an ordinary app for as long
  as the window is open — a Dock icon appears — and goes back to living only in the menu
  bar when you close it.

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

## Setting up a machine

    collab setup

Asks the one question everything else follows from — is this machine the server, or does
it talk to one — and then proves the answer before saying it is done.

On the server it makes a channel, offers to start the server at login, and prints the
three things the other machine needs: an address, a channel name, and its key.

On a client it asks for that address and **checks it there and then**, before anything is
written. This is the failure worth catching early: a wrong address does not produce an
error, it produces silence, and silence is exactly what an empty channel looks like. The
server's greeting is sent in the clear before any key is involved, so reachability can be
checked before the key is even typed; the key is then checked separately by actually
reading the channel.

A name ending in `.local` often does not resolve from Windows. Setup says so when it
fails, rather than leaving you to guess.

## The disk image

    ./dmg.sh

Produces `dist.noindex/collab.dmg` — open it, drag Collab.app to Applications, done.
The command-line half travels inside the app, so there is nothing else to install and
nothing left behind if it is dragged to the bin. Open the app afterwards and run
`collab setup`, or point it at a server.

The window is laid out with the app on the left and Applications on the right. The
custom background does not currently render on macOS 26 — it is set without error and
recorded, and still does not appear. The image works regardless; it just looks plain.

## Build and install

    ./build.sh      # everything, into dist.noindex/
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

Then make a channel with the `#` button in the app, and `collab test-notify` once. macOS
asks whether to allow notifications from "collab" the first time; say yes.

**MCP** — register the core once per machine, in `~/.claude.json` on the Mac or
`%USERPROFILE%\.claude.json` on Windows:

    "mcpServers": { "collab": { "command": "/Users/you/.local/bin/collab", "args": ["mcp"] } }

An absolute path on purpose: the server is spawned by the app, which does not
necessarily have `~/.local/bin` on its PATH. No `env` block is needed — name,
channel and key all come from `~/.collab-config`, and the core looks at `HOME` and
then `USERPROFILE`, so the same arrangement works on both machines.

## Updates

There is a **Check for Updates…** item in the menu bar. It fetches a release, checks
its signature, tells you what would change, and installs only if you say so.

The dialog is not what makes this safe. It asks you to approve something you cannot
inspect, and you would click through it for somebody else's build as readily as for
your own. What makes it safe is that **the release is signed by a key that lives
nowhere near where it is published**: the public key is compiled into collab, the
private key sits in a password manager, and anything that does not verify is refused
before you are asked. Taking over the account the release is published from is not
enough.

Nothing is checked or downloaded until both halves exist:

    update_url = https://github.com/Artificial-IntelligenceAI/collab/releases/latest/download
    PUBLIC_KEY                         # in core/src/release.rs

Check the pair once, when it is first set up:

    mkdir -p /tmp/x && echo hi > /tmp/x/hi.txt
    collab release sign /tmp/x -version 0.0.1 -key -
    collab release verify /tmp/x

A public key that does not match the private one is still a perfectly valid key. It
would simply refuse every release for ever, and the first you would know is the day you
needed an update to work.

Making a release:

    collab release keygen              # once, ever — private key into your password manager
    ./release.sh 3.1.0 "what changed"  # builds, then asks for the key on stdin

The key is read from stdin rather than an argument, because an argument is visible in
the process list while it runs and in shell history afterwards. Every file is hashed
into a manifest, the manifest is signed, and the update checks each file against it —
so a release where one artefact was swapped fails on that artefact, not merely at the
front door.

If the private key is ever lost, updates stop until you hand-deliver a build carrying a
new public key. If it leaks, do the same, urgently.

## Windows

Not finished, but it builds. The Rust core cross-compiles once the toolchain is there:

    rustup target add x86_64-pc-windows-gnu && brew install mingw-w64

Verified rather than assumed: that produces a `PE32+ executable (console) x86-64`,
and `build.sh` stops skipping the Windows half. `notify/windows` is the C# toast
helper and builds too.

Both have now been **run**, on a Windows 11 ARM VM, and the following are
verified rather than assumed: messages in both directions across a real network,
encrypted; channel keys carried between machines; live streaming with backlog
marked as backlog; resume after an outage; `@` mentions and their refusals;
change entries; the users listing; and file transfer both ways, hash-identical
at each end. Toast notifications work — confirmed on screen, not by an exit code.

Running it found two bugs that could not have been found any other way: a
P/Invoke to a function that is not an export, which had stopped notifications
from ever working; and a watcher that recorded its place before delivering,
which silently lost a message whenever its reader went away.

**Installing on Windows.** `./windows-setup.sh` builds `collab-setup.zip` — the
app, the command line in `bin/`, the icon and a readable installer. Hand it over;
they extract it and double-click `Install.cmd`. It installs per user under
`%LOCALAPPDATA%\Programs\Collab`, so it never asks for an administrator, makes
Start Menu and Desktop shortcuts, and appears in Installed Apps.

The uninstaller leaves `~/.collab-*` alone. Those are channel keys and message
history, and removing a program is not consent to delete the conversations.

Verified on a Windows 11 ARM VM as a genuine first install and a full removal:
files, both shortcuts, the Installed Apps entry, the command line running from
where it landed, and — after uninstalling — everything gone except the keys.

## Files

    build.sh    builds both machines' worth of it into dist.noindex/
    install.sh  installs this Mac's half, and upgrades it
    release.sh  builds and signs a release
    com.tankun.collab.plist      LaunchAgent for the server

    core/src/main.rs     command dispatch
    core/src/config.rs   settings, ~/.collab-config, `collab who`
    core/src/channels.rs channels and their keys
    core/src/files.rs    the file store, kept by content
    core/src/release.rs  signing a release, and verifying one before it is installed
    core/src/crypto.rs   frame sealing
    core/src/wire.rs     the connection: a challenge, then nothing in the clear
    core/src/server.rs   the hub: sequence numbers, subscribers, replay-on-connect
    core/src/client.rs   watch, post, change, log, and the reconnect rule
    core/src/history.rs  the file the server owns, and purging a channel from it
    core/src/notify.rs   finding the notifier, and coalescing bursts into one popup
    core/src/mcp.rs      the MCP tools
    core/src/msg.rs      what travels on the wire

    app/mac/Sources/     the menu bar app, its window, and the channel panel
    app/mac/icon.swift   generates the icon both platforms use
    notify/windows/      C# toast helper (unverified)

## License

Apache License 2.0 — see [LICENSE](LICENSE). Copyright 2026 Tankun Sriket.
