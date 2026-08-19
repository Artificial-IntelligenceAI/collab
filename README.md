# collab

A message channel between two machines on the same Wi-Fi, so two Claude sessions
can tell each other what they just did.

    collab serve              run the server (one machine only)
    collab watch              stream messages — this is what Monitor runs
    collab post "message"     send one
    collab log                history
    collab mcp                run as an MCP server (tools only)

Env: `COLLAB_HOST`, `COLLAB_PORT` (8787), `COLLAB_NAME`, `COLLAB_CHANNEL` (general)

## Why it works the way it does

**Every message has a sequence number, and a watcher remembers the last one it saw.**
On reconnect it asks the server to resume from there, so a message sent while you
were offline still arrives — exactly once. A dropped message and a quiet channel
must never look the same.

**Disconnection is announced.** `* DISCONNECTED — retrying`, then `* reconnected,
resuming from #N`. Silence should mean nobody is talking, never that the wire died.

**The server survives restarts** — it reads the history file and resumes numbering.

**MCP is tools only, no resources, no subscriptions.** Tested on 2026-08-19: a server
pushed 25 notifications over 8 minutes, via both `notifications/resources/updated`
and `notifications/message`, and Claude Desktop never subscribed and never reacted.
It is a pull-only client. So notifications come from `Monitor` running `collab watch`,
and MCP only makes posting a typed tool call instead of a shell command.

## Setup

**Mac (server side)**

    cp collab-macos-arm64 ~/.local/bin/collab
    sudo ln -sf ~/.local/bin/collab /usr/local/bin/collab      # optional, for PATH
    cp com.tankun.collab.plist ~/Library/LaunchAgents/
    launchctl load ~/Library/LaunchAgents/com.tankun.collab.plist

**Windows (her side)** — copy `collab.exe`, then set:

    setx COLLAB_HOST Tankuns-MacBook-Pro.local
    setx COLLAB_NAME sis

If `.local` doesn't resolve, use the Mac's LAN address instead.

**Both sides, per project:**

    setx COLLAB_CHANNEL roblox        # or: export COLLAB_CHANNEL=roblox

**MCP registration** (optional, for the typed tools) — add to the project's
`mcpServers`:

    { "collab": { "command": "collab", "args": ["mcp"] } }
