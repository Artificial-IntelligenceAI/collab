# collab

A message channel between two machines on the same Wi-Fi, so two Claude sessions
can tell each other what they just did.

    collab serve              run the server (one machine only)
    collab watch              stream messages — this is what Monitor runs
    collab post "message"     send a chat message
    collab change ...         record a structured change (see below)
    collab log [-changes]     history
    collab gui                open the window — chat, changes, search
    collab who                show the name, channel and server in use
    collab test-notify        check that popup notifications work
    collab mcp                run as an MCP server (tools only)

Env: `COLLAB_HOST`, `COLLAB_PORT` (8787), `COLLAB_NAME`, `COLLAB_CHANNEL` (general),
`COLLAB_GUI_PORT` (8788), `COLLAB_NOTIFY` (`0` turns popups off)

## Who you are

A name is `COLLAB_NAME`, or failing that your computer's hostname. There are no
accounts and nothing to sign up for.

Settings are read from the environment first, then `~/.collab-config`, then the
default:

    name    = tankun
    channel = roblox

The file is not just a convenience. A collab started by the MCP server, by launchd,
or by clicking a notification inherits none of your shell, so anything set only in
`.zshrc` works in a terminal and quietly fails everywhere else — posting under your
hostname, on the wrong channel. `collab who` says what is actually in force, and
where each answer came from.

**A name belongs to a machine, not a person** — which means it covers you *and*
your Claude. So every message also records whether a person or an AI sent it, and
shows as `sis` or `sis's AI`. Anything sent through the MCP tools is the AI; the
window's text box is you; a command typed in a terminal is taken to be you.

That distinction is the point of the whole tool: "sis is asking you something" and
"sis's AI edited a script" deserve different reactions, and until now both said
just "sis".

## Two kinds of message

**chat** is free text — "I'm in the shop scripts, leave them alone for a bit".

**change** is structured: who, which script or instance, what kind of change, and a
one-line summary.

    collab change -action edited -target "ServerScriptService/ShopHandler" \
        "gave the buy button a debounce so double-clicks stop double-charging"

`-action` is one of `added`, `edited`, `removed`, `renamed`, and nothing else is
accepted. Both kinds share the sequence numbering and the same channel, so nothing
is lost either way — the Changes view just filters to the second kind.

Recording a change is a **deliberate act**, never something parsed out of prose. A
change record inferred from someone's sentence is a guess wearing a fact's clothes,
and this whole tool exists so neither AI has to guess what the other did.

## The window

    collab gui

Opens a page in your browser at `http://127.0.0.1:8788` — bound to your own machine
only, never to the network.

- **Chat** — everything, in order, with day separators and a box to type in.
- **Changes** — *a git log for a project that can't use git.* Roblox saves a binary
  `.rbxl`, so git has nothing to show. But the AIs know what they changed, because
  they made the changes. Consecutive changes by one person within 15 minutes are
  grouped into one entry, the way `git log` groups edits into a commit.
- **Search** and a **channel picker** apply to both views. You post where you are
  looking: switch the picker and the composer follows, because reading one channel
  and typing into another would be a nasty little trap.
- Solarized, light and dark, following whatever the OS is set to.

The window is a viewer plus a chat box; it is not where changes get recorded. That
is the AI's job, through `collab change` or the MCP tool.

## Notifications

Real ones — collab's own name and icon, in Notification Centre on the Mac and the
Action Centre on Windows, exactly like any other app. They come from `collab watch`,
which Monitor is already running, so they arrive with the window closed and the
browser shut.

    collab test-notify        # check they work, without waiting for anyone to speak

Neither platform will let a plain Go binary raise one. macOS attributes a
notification to an **application bundle**; Windows attributes a toast to a
registered **AppUserModelID**. Anything that dodges this — `osascript` on the Mac,
PowerShell on Windows — pops up under somebody else's name. So there are two small
helpers, built from source in `notify/`:

- **`collab.app`** — Swift, ad-hoc signed with a stable identifier so the permission
  you grant survives rebuilds. Lives next to `collab`, or in `~/Applications`.
- **`collab-notify.exe`** — C#, self-contained, so nothing has to be installed on
  the Windows machine. That is what makes it 94 MB: a framework-dependent build is
  24 MB but requires a Microsoft runtime install, and "no runtime on either machine"
  is the whole reason this project is written in Go. It registers a Start Menu
  shortcut and a registry entry on first run. Lives next to `collab.exe`, along
  with `collab.png`, which is the icon the toast shows.

The Windows helper is **built but not verified** — it cross-compiles from the Mac,
but there is no Windows machine here to run it on, and toast registration is the
fiddly part. `collab test-notify` on her machine is the check; if it prints an
error, that error is the thing to fix.

**Clicking one opens the window, on the channel the message came from** — the way
clicking a WhatsApp notification opens that conversation rather than the app in
general. If the window is already open it comes forward; if it is closed it gets
started. A window opened this way inherits none of your shell's `COLLAB_` settings,
so the click hands them over explicitly — otherwise it would open on the default
channel and quietly post to the wrong one.

**Your own AI gets a popup; you do not.** Your own typing is never announced back
at you, but your Claude's messages are — you are usually looking at Roblox Studio,
not at the session.

**A burst is one popup, not forty.** When a machine wakes after being asleep the
server replays everything it missed at once, and forty popups in a row is not a
notification, it is a punishment. Arrivals are collected until the channel goes
quiet for 700 ms: one message becomes a detailed popup, a burst becomes a summary
("sis · 6 new on #roblox · 6 changes"). Your own messages never pop.

Popups come from `collab watch`. The window can raise them too — `collab gui
-notify` — but that is off by default, because you would otherwise get each one
twice. `COLLAB_NOTIFY=0` turns them off entirely.

## Why it works the way it does

**Every message has a sequence number, and a watcher remembers the last one it saw.**
On reconnect it asks the server to resume from there, so a message sent while you
were offline still arrives — exactly once. A dropped message and a quiet channel
must never look the same. If the watcher cannot even write down its place, it says
so on stderr rather than silently replaying history later.

**Disconnection is announced.** `* DISCONNECTED — retrying`, then `* reconnected,
resuming from #N`. The window says the same thing in a red banner and a green one.
Silence should mean nobody is talking, never that the wire died. When a `post` or a
tool call can't reach the server it reports the failure loudly — it never claims to
have sent something that went nowhere.

**The server survives restarts** — it reads the history file and resumes numbering.

**The server owns the only complete history.** The other machine asks for it over
the wire (`collab log`, `collab_recent`, `collab_changes`), and falls back to its
own local copy only if the server is unreachable.

**MCP is tools only, no resources, no subscriptions.** Tested on 2026-08-19: a server
pushed 25 notifications over 8 minutes, via both `notifications/resources/updated`
and `notifications/message`, and Claude Desktop never subscribed and never reacted.
It is a pull-only client. So notifications come from `Monitor` running `collab watch`,
and MCP only makes posting and recording typed tool calls instead of shell commands.

**No dependencies.** `go.mod` lists nothing; there is no `go.sum`. That is what keeps
the Windows build a single command with nothing to install on either machine.

## Build

    ./build.sh

Puts everything in `dist/macos` and `dist/windows`. The Go binaries need only Go;
`collab.app` needs Xcode's Swift; `collab-notify.exe` needs the .NET SDK
(`brew install dotnet`) and is skipped with a warning if it is missing, rather than
quietly shipping a Windows build with no popups in it.

Just the Go parts, if that is all you changed:

    GOOS=darwin  GOARCH=arm64 go build -o dist/macos/collab .
    GOOS=windows GOARCH=amd64 go build -o dist/windows/collab.exe .

## Setup

**Mac (server side)**

    ./install.sh

Puts the binary in `~/.local/bin`, the notifier in `~/Applications`, and loads the
LaunchAgent so the server starts at login and restarts if it dies. Safe to re-run;
that is also how you upgrade.

Then `collab test-notify` once. macOS asks whether to allow notifications from
"collab" the first time, the same as any app; say yes.

Do not install by copying over the old binary yourself. Writing over a Mach-O file
in place leaves macOS holding a stale code signature for it, and the kernel then
kills it on sight **with no error message at all** — the command simply dies. That
is why `install.sh` deletes before it copies, and why you should use it.

The LaunchAgent runs `collab serve` only. Open the window yourself with `collab gui`
when you want to look at it.

**Windows (her side)** — copy the whole of `dist/windows` (`collab.exe`,
`collab-notify.exe`, `collab.png`) into one folder, keeping them together — the
notifier is found next to `collab.exe`. Then set:

    setx COLLAB_HOST Tankuns-MacBook-Pro.local
    setx COLLAB_NAME sis

If `.local` doesn't resolve, use the Mac's LAN address instead.

**Both sides, per project:**

    setx COLLAB_CHANNEL roblox        # or: export COLLAB_CHANNEL=roblox

Then `collab test-notify` on each machine.

**MCP registration** (optional, for the typed tools) — add to the project's
`mcpServers`:

    { "collab": { "command": "collab", "args": ["mcp"] } }

Tools: `collab_post`, `collab_change`, `collab_recent`, `collab_changes`.

## Files

    build.sh    builds both machines' worth of it into dist/
    install.sh  installs this Mac's half, and upgrades it
    main.go     types, history file, command dispatch
    server.go   the hub: sequence numbers, subscribers, replay-on-connect
    client.go   watch, post, change, log, and the reconnect rule
    gui.go      the local web server behind `collab gui`
    ui.html     the window itself, embedded into the binary
    notify.go   finding the platform helper, and coalescing bursts into one popup
    mcp.go      the MCP tools

    notify/mac/       Swift source for collab.app, and the icon generator
    notify/windows/   C# source for collab-notify.exe
