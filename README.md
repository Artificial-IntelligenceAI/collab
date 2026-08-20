# collab

A message channel between two machines on the same Wi-Fi, so two Claude sessions
can tell each other what they just did.

    collab serve              run the server (one machine only)
    collab watch              stream messages — this is what Monitor runs
    collab post "message"     send a chat message
    collab change ...         record a structured change (see below)
    collab log [-changes]     history
    collab gui                open the window — chat, changes, search
    collab mcp                run as an MCP server (tools only)

Env: `COLLAB_HOST`, `COLLAB_PORT` (8787), `COLLAB_NAME`, `COLLAB_CHANNEL` (general),
`COLLAB_GUI_PORT` (8788)

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
- **Search** and a **channel picker** apply to both views.
- Solarized, light and dark, following whatever the OS is set to.

The window is a viewer plus a chat box; it is not where changes get recorded. That
is the AI's job, through `collab change` or the MCP tool.

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

    go build -o collab-macos-arm64 .
    GOOS=windows GOARCH=amd64 go build -o collab.exe .

## Setup

**Mac (server side)**

    cp collab-macos-arm64 ~/.local/bin/collab
    sudo ln -sf ~/.local/bin/collab /usr/local/bin/collab      # optional, for PATH
    cp com.tankun.collab.plist ~/Library/LaunchAgents/
    launchctl load ~/Library/LaunchAgents/com.tankun.collab.plist

The LaunchAgent runs `collab serve` only. Open the window yourself with `collab gui`
when you want to look at it.

**Windows (her side)** — copy `collab.exe`, then set:

    setx COLLAB_HOST Tankuns-MacBook-Pro.local
    setx COLLAB_NAME sis

If `.local` doesn't resolve, use the Mac's LAN address instead.

**Both sides, per project:**

    setx COLLAB_CHANNEL roblox        # or: export COLLAB_CHANNEL=roblox

**MCP registration** (optional, for the typed tools) — add to the project's
`mcpServers`:

    { "collab": { "command": "collab", "args": ["mcp"] } }

Tools: `collab_post`, `collab_change`, `collab_recent`, `collab_changes`.

## Files

    main.go     types, history file, command dispatch
    server.go   the hub: sequence numbers, subscribers, replay-on-connect
    client.go   watch, post, change, log, and the reconnect rule
    gui.go      the local web server behind `collab gui`
    ui.html     the window itself, embedded into the binary
    mcp.go      the MCP tools
