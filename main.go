// collab — a message channel between two machines on the same network, so two AI
// sessions can tell each other what they just did.
//
//	collab serve                 run the server (one machine only)
//	collab watch                 stream messages as they arrive
//	collab post "message"        send a chat message
//	collab change ...            record a structured change
//	collab log                   print history
//	collab gui                   open the window (chat + changes)
//	collab mcp                   run as an MCP server (tools, not notifications)
//
// Env: COLLAB_HOST, COLLAB_PORT (8787), COLLAB_NAME, COLLAB_CHANNEL (general),
//
//	COLLAB_GUI_PORT (8788)
//
// Every message carries a sequence number, and a watcher remembers the last one
// it saw. On reconnect it asks to resume from there, so a message is never
// silently lost — a dropped message and a quiet channel must not look the same.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

// A message is one of two kinds. Both share the sequence numbering and the same
// channel, so nothing is lost either way — but a change is structured, because a
// change record inferred from prose is a guess wearing a fact's clothes.
const (
	KindChat   = "chat"
	KindChange = "change"
)

// What a change did. Anything else is rejected at the door.
var actions = []string{"added", "edited", "removed", "renamed"}

type Msg struct {
	Seq     int64  `json:"seq"`
	Channel string `json:"channel"`
	From    string `json:"from"`
	At      string `json:"at"`
	Kind    string `json:"kind,omitempty"` // "" (old records) means chat
	Text    string `json:"text"`           // chat body, or the change's one-line summary

	// change only
	Action string `json:"action,omitempty"` // added | edited | removed | renamed
	Target string `json:"target,omitempty"` // which script or instance
}

func (m Msg) kind() string {
	if m.Kind == KindChange {
		return KindChange
	}
	return KindChat // records written by v1 have no kind at all
}

// One line, for a terminal — this is what Monitor ends up showing.
func (m Msg) line() string {
	if m.kind() == KindChange {
		if m.Target != "" {
			return fmt.Sprintf("[%s] %s — %s", m.Action, m.Target, m.Text)
		}
		return fmt.Sprintf("[%s] %s", m.Action, m.Text)
	}
	return m.Text
}

type Hello struct {
	Name    string `json:"name"`
	Channel string `json:"channel"`
	Since   int64  `json:"since"`
	Mode    string `json:"mode"` // "watch" | "post" | "fetch"
}

func env(k, def string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return def
}

func home(name string) string {
	h, err := os.UserHomeDir()
	if err != nil {
		h = "."
	}
	return filepath.Join(h, name)
}

func hostName() string {
	h, _ := os.Hostname()
	return strings.TrimSuffix(h, ".local")
}

var (
	addr     = func() string { return env("COLLAB_HOST", "localhost") + ":" + env("COLLAB_PORT", "8787") }
	name     = func() string { return env("COLLAB_NAME", hostName()) }
	channel  = func() string { return env("COLLAB_CHANNEL", "general") }
	histPath = home(".collab-history.jsonl")
	seenPath = home(".collab-seen")
)

// ───────────────────────────── history ─────────────────────────────

var histMu sync.Mutex

func appendHistory(m Msg) {
	histMu.Lock()
	defer histMu.Unlock()
	f, err := os.OpenFile(histPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	b, _ := json.Marshal(m)
	f.Write(append(b, '\n'))
}

func readHistory() []Msg {
	histMu.Lock()
	defer histMu.Unlock()
	f, err := os.Open(histPath)
	if err != nil {
		return nil
	}
	defer f.Close()
	var out []Msg
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)
	for sc.Scan() {
		var m Msg
		if json.Unmarshal(sc.Bytes(), &m) == nil {
			out = append(out, m)
		}
	}
	return out
}

func filterHistory(in []Msg, ch string, since int64) []Msg {
	var out []Msg
	for _, m := range in {
		if m.Seq > since && (ch == "" || m.Channel == ch) {
			out = append(out, m)
		}
	}
	return out
}

// ───────────────────────────── entry ─────────────────────────────

const usage = `usage:
  collab serve                          run the server (one machine only)
  collab watch                          stream messages — this is what Monitor runs
  collab post "message"                 send a chat message
  collab change -action edited -target "ServerScriptService/Shop" "what changed"
  collab log [-changes]                 history
  collab gui [-no-open] [-notify]        open the window
  collab test-notify                    check that popup notifications work
  collab mcp                            run as an MCP server`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, usage)
		os.Exit(2)
	}
	switch os.Args[1] {
	case "serve":
		serve()
	case "watch":
		watch()
	case "post":
		post(strings.Join(os.Args[2:], " "))
	case "change":
		changeCmd(os.Args[2:])
	case "log":
		showLog(os.Args[2:])
	case "gui":
		runGUI(os.Args[2:])
	case "test-notify":
		testNotify()
	case "mcp":
		runMCP()
	default:
		fmt.Fprintln(os.Stderr, usage)
		os.Exit(2)
	}
}
