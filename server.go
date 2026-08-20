// The server. One machine runs it; everyone else dials in.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"strings"
	"sync"
	"time"
)

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
	fmt.Printf("[%s] #%d %s: %s\n", m.Channel, m.Seq, m.who(), m.line())
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

	switch hello.Mode {
	case "fetch":
		// History in one shot, then hang up. For `collab log` and the MCP tools
		// on the machine that isn't the server — its own history file is empty.
		for _, m := range filterHistory(readHistory(), hello.Channel, hello.Since) {
			enc.Encode(m)
		}
		return

	case "watch":
		// Everything the watcher missed, then live. No gap, by construction.
		for _, m := range filterHistory(readHistory(), hello.Channel, hello.Since) {
			enc.Encode(m)
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

	// post: every remaining line is a message. A line of JSON is a structured
	// message; anything else is plain chat text, which is what v1 clients send.
	for r.Scan() {
		line := strings.TrimSpace(r.Text())
		if line == "" {
			continue
		}
		var m Msg
		if !strings.HasPrefix(line, "{") || json.Unmarshal([]byte(line), &m) != nil {
			m = Msg{Text: line}
		}
		if strings.TrimSpace(m.Text) == "" && m.Target == "" {
			continue
		}
		// The connection says who and where; the payload does not get a vote.
		m.Seq = 0
		m.Channel = hello.Channel
		m.From = hello.Name
		m.At = time.Now().Format(time.RFC3339)
		if m.Kind != KindChange {
			m.Kind = KindChat
			m.Action, m.Target = "", ""
		}
		if m.Via != ActorAI {
			m.Via = "" // a person, or something that did not say
		}
		h.publish(m)
	}
}
