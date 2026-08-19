// collab — a small message channel between two machines on the same network,
// so two AI sessions can tell each other what they just did.
//
//	collab serve                 run the server (one machine only)
//	collab watch                 stream messages as they arrive
//	collab post "message"        send one message
//	collab log                   print history
//	collab mcp                   run as an MCP server (tools, not notifications)
//
// Env: COLLAB_HOST, COLLAB_PORT (8787), COLLAB_NAME, COLLAB_CHANNEL (general)
//
// Every message carries a sequence number, and a watcher remembers the last one
// it saw. On reconnect it asks to resume from there, so a message is never
// silently lost — a dropped message and a quiet channel must not look the same.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

type Msg struct {
	Seq     int64  `json:"seq"`
	Channel string `json:"channel"`
	From    string `json:"from"`
	At      string `json:"at"`
	Text    string `json:"text"`
}

type Hello struct {
	Name    string `json:"name"`
	Channel string `json:"channel"`
	Since   int64  `json:"since"`
	Mode    string `json:"mode"` // "watch" or "post"
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

// ───────────────────────────── server ─────────────────────────────

type sub struct {
	ch      chan Msg
	channel string
}

type hub struct {
	mu   sync.Mutex
	subs map[*sub]bool
	seq  int64
}

func (h *hub) publish(m Msg) {
	h.mu.Lock()
	h.seq++
	m.Seq = h.seq
	subs := make([]*sub, 0, len(h.subs))
	for s := range h.subs {
		subs = append(subs, s)
	}
	h.mu.Unlock()

	appendHistory(m)
	for _, s := range subs {
		if s.channel != "" && s.channel != m.Channel {
			continue
		}
		select {
		case s.ch <- m:
		default: // a stalled reader must not block everyone else
		}
	}
	fmt.Printf("[%s] #%d %s: %s\n", m.Channel, m.Seq, m.From, m.Text)
}

func serve() {
	h := &hub{subs: map[*sub]bool{}}
	for _, m := range readHistory() { // resume numbering across restarts
		if m.Seq > h.seq {
			h.seq = m.Seq
		}
	}

	ln, err := net.Listen("tcp", ":"+env("COLLAB_PORT", "8787"))
	if err != nil {
		fmt.Fprintf(os.Stderr, "collab: %v\n", err)
		os.Exit(1)
	}
	hn, _ := os.Hostname()
	fmt.Printf("collab server on %s (port %s), resuming at #%d\n", hn, env("COLLAB_PORT", "8787"), h.seq)
	fmt.Printf("others use:  COLLAB_HOST=%s collab watch\n", hn)

	for {
		c, err := ln.Accept()
		if err != nil {
			continue
		}
		go handle(h, c)
	}
}

func handle(h *hub, c net.Conn) {
	defer c.Close()
	if t, ok := c.(*net.TCPConn); ok {
		t.SetKeepAlive(true)
		t.SetKeepAlivePeriod(15 * time.Second)
	}
	r := bufio.NewScanner(c)
	r.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	if !r.Scan() {
		return
	}
	var hello Hello
	if err := json.Unmarshal(r.Bytes(), &hello); err != nil {
		return
	}

	enc := json.NewEncoder(c)

	if hello.Mode == "watch" {
		// Everything the watcher missed, then live. No gap, by construction.
		for _, m := range readHistory() {
			if m.Seq > hello.Since && (hello.Channel == "" || m.Channel == hello.Channel) {
				enc.Encode(m)
			}
		}
		s := &sub{ch: make(chan Msg, 256), channel: hello.Channel}
		h.mu.Lock()
		h.subs[s] = true
		h.mu.Unlock()
		defer func() { h.mu.Lock(); delete(h.subs, s); h.mu.Unlock() }()

		done := make(chan struct{})
		go func() {
			for r.Scan() {
			}
			close(done)
		}() // notice the peer leaving
		for {
			select {
			case m := <-s.ch:
				if enc.Encode(m) != nil {
					return
				}
			case <-done:
				return
			}
		}
	}

	// post: every remaining line is a message
	for r.Scan() {
		line := strings.TrimSpace(r.Text())
		if line == "" {
			continue
		}
		h.publish(Msg{Channel: hello.Channel, From: hello.Name, At: time.Now().Format(time.RFC3339), Text: line})
	}
}

// ───────────────────────────── client ─────────────────────────────

func lastSeen() int64 {
	b, err := os.ReadFile(seenPath)
	if err != nil {
		return 0
	}
	n, _ := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64)
	return n
}

func saveSeen(n int64) { os.WriteFile(seenPath, []byte(strconv.FormatInt(n, 10)), 0o644) }

func watch() {
	announced := false
	for {
		c, err := net.Dial("tcp", addr())
		if err != nil {
			if !announced {
				// A dead channel must never be mistaken for a quiet one.
				fmt.Printf("* DISCONNECTED from %s — retrying\n", addr())
				announced = true
			}
			time.Sleep(2 * time.Second)
			continue
		}
		if announced {
			fmt.Printf("* reconnected to %s, resuming from #%d\n", addr(), lastSeen())
			announced = false
		}
		json.NewEncoder(c).Encode(Hello{Name: name(), Channel: channel(), Since: lastSeen(), Mode: "watch"})

		sc := bufio.NewScanner(c)
		sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
		for sc.Scan() {
			var m Msg
			if json.Unmarshal(sc.Bytes(), &m) != nil {
				continue
			}
			fmt.Printf("[%s] %s: %s\n", m.Channel, m.From, m.Text)
			saveSeen(m.Seq)
		}
		c.Close()
		if !announced {
			fmt.Printf("* DISCONNECTED from %s — retrying\n", addr())
			announced = true
		}
		time.Sleep(2 * time.Second)
	}
}

func post(text string) {
	if strings.TrimSpace(text) == "" {
		fmt.Fprintln(os.Stderr, `usage: collab post "message"`)
		os.Exit(2)
	}
	c, err := net.Dial("tcp", addr())
	if err != nil {
		fmt.Fprintf(os.Stderr, "collab: cannot reach %s — %v\n", addr(), err)
		os.Exit(1)
	}
	defer c.Close()
	json.NewEncoder(c).Encode(Hello{Name: name(), Channel: channel(), Mode: "post"})
	fmt.Fprintln(c, strings.ReplaceAll(text, "\n", " "))
	time.Sleep(200 * time.Millisecond)
}

func showLog() {
	for _, m := range readHistory() {
		if m.Channel == channel() || channel() == "" {
			fmt.Printf("#%-4d [%s] %s: %s\n", m.Seq, m.At[11:16], m.From, m.Text)
		}
	}
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, `usage: collab serve | watch | post "msg" | log | mcp`)
		os.Exit(2)
	}
	switch os.Args[1] {
	case "serve":
		serve()
	case "watch":
		watch()
	case "post":
		post(strings.Join(os.Args[2:], " "))
	case "log":
		showLog()
	case "mcp":
		runMCP()
	default:
		fmt.Fprintln(os.Stderr, `usage: collab serve | watch | post "msg" | log | mcp`)
		os.Exit(2)
	}
}
