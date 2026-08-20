// The MCP side. We proved a server cannot push anything into a session, so this
// deliberately offers only tools — pulling. The push still comes from `collab
// watch` running under a Monitor.
//
// Tested 2026-08-19: a server pushed 25 notifications over 8 minutes via both
// notifications/resources/updated and notifications/message, and the client never
// subscribed and never reacted. Advertising a capability that does nothing would
// be a lie in the handshake, so we advertise tools and nothing else.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"slices"
	"strings"
)

type rpc struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method,omitempty"`
	Params  json.RawMessage `json:"params,omitempty"`
	Result  any             `json:"result,omitempty"`
	Error   *rpcErr         `json:"error,omitempty"`
}

type rpcErr struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func obj(m map[string]any) map[string]any { return m }

func schema(props map[string]any, required ...string) map[string]any {
	s := map[string]any{"type": "object", "properties": props}
	if len(required) > 0 {
		s["required"] = required
	}
	return s
}

func str(desc string) map[string]any { return map[string]any{"type": "string", "description": desc} }

func tools() []any {
	return []any{
		obj(map[string]any{
			"name": "collab_post",
			"description": "Send a chat message to the other person's Claude on the shared channel. " +
				"Use it to say what you are about to touch, to ask them something, or to answer them. " +
				"For recording something you actually changed, use collab_change instead.",
			"inputSchema": schema(map[string]any{
				"message": str("What to tell them."),
			}, "message"),
		}),
		obj(map[string]any{
			"name": "collab_change",
			"description": "Record something you just changed, as a structured entry. This is what fills the Changes " +
				"view — a git log for a project that cannot use git, because Roblox saves a binary .rbxl. " +
				"Call it right after you make a change, once per script or instance you touched. " +
				"Only record what you actually did; never infer an entry from what someone said.",
			"inputSchema": schema(map[string]any{
				"action":  map[string]any{"type": "string", "enum": actions, "description": "What you did: added, edited, removed or renamed."},
				"target":  str("Which script or instance, as a path — e.g. ServerScriptService/ShopHandler."),
				"summary": str("One line, past tense, what changed — e.g. 'gave the buy button a debounce'."),
			}, "action", "target", "summary"),
		}),
		obj(map[string]any{
			"name":        "collab_recent",
			"description": "Read recent activity on the shared channel, oldest first — both chat and recorded changes.",
			"inputSchema": schema(map[string]any{
				"count": map[string]any{"type": "integer", "description": "How many entries (default 20)."},
				"kind":  map[string]any{"type": "string", "enum": []string{"all", "chat", "change"}, "description": "Which kind to show (default all)."},
			}),
		}),
		obj(map[string]any{
			"name": "collab_changes",
			"description": "Read the recorded changes on the shared channel, newest first, grouped by who made them — " +
				"the same thing the Changes view shows. Read this before touching a script, to see if the other " +
				"session has already been in it.",
			"inputSchema": schema(map[string]any{
				"count": map[string]any{"type": "integer", "description": "How many changes (default 20)."},
			}),
		}),
	}
}

func runMCP() {
	out := json.NewEncoder(os.Stdout)
	reply := func(id json.RawMessage, res any) {
		if id == nil {
			return
		}
		out.Encode(rpc{JSONRPC: "2.0", ID: id, Result: res})
	}
	text := func(s string) any {
		return map[string]any{"content": []any{map[string]string{"type": "text", "text": s}}}
	}

	sc := bufio.NewScanner(os.Stdin)
	sc.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)
	for sc.Scan() {
		var m rpc
		if json.Unmarshal(sc.Bytes(), &m) != nil {
			continue
		}
		switch m.Method {
		case "initialize":
			var p struct {
				ProtocolVersion string `json:"protocolVersion"`
			}
			json.Unmarshal(m.Params, &p)
			if p.ProtocolVersion == "" {
				p.ProtocolVersion = "2025-06-18"
			}
			reply(m.ID, map[string]any{
				"protocolVersion": p.ProtocolVersion,
				// No resources, no subscriptions — they demonstrably do nothing.
				"capabilities": map[string]any{"tools": map[string]any{}},
				"serverInfo":   map[string]string{"name": "collab", "version": "2.0.0"},
			})

		case "tools/list":
			reply(m.ID, map[string]any{"tools": tools()})

		case "tools/call":
			var p struct {
				Name string `json:"name"`
				Args struct {
					Message string `json:"message"`
					Action  string `json:"action"`
					Target  string `json:"target"`
					Summary string `json:"summary"`
					Kind    string `json:"kind"`
					Count   int    `json:"count"`
				} `json:"arguments"`
			}
			json.Unmarshal(m.Params, &p)

			switch p.Name {
			case "collab_post":
				msg := strings.ReplaceAll(strings.TrimSpace(p.Args.Message), "\n", " ")
				if msg == "" {
					reply(m.ID, text("nothing to send — message was empty"))
					break
				}
				if err := send(Msg{Kind: KindChat, Text: msg}); err != nil {
					reply(m.ID, text(fmt.Sprintf("could not reach the collab server at %s (%v) — the other session did NOT get this", addr(), err)))
					break
				}
				reply(m.ID, text("sent: "+msg))

			case "collab_change":
				action := strings.ToLower(strings.TrimSpace(p.Args.Action))
				target := strings.TrimSpace(p.Args.Target)
				summary := strings.ReplaceAll(strings.TrimSpace(p.Args.Summary), "\n", " ")
				if !slices.Contains(actions, action) {
					reply(m.ID, text("action must be one of: "+strings.Join(actions, ", ")))
					break
				}
				if target == "" || summary == "" {
					reply(m.ID, text("a change needs both a target and a one-line summary"))
					break
				}
				if err := send(Msg{Kind: KindChange, Action: action, Target: target, Text: summary}); err != nil {
					reply(m.ID, text(fmt.Sprintf("could not reach the collab server at %s (%v) — the change was NOT recorded", addr(), err)))
					break
				}
				reply(m.ID, text(fmt.Sprintf("recorded: %s %s — %s", action, target, summary)))

			case "collab_recent":
				n := p.Args.Count
				if n <= 0 {
					n = 20
				}
				h := fetch(channel(), 0)
				if k := p.Args.Kind; k == KindChat || k == KindChange {
					var f []Msg
					for _, msg := range h {
						if msg.kind() == k {
							f = append(f, msg)
						}
					}
					h = f
				}
				if len(h) > n {
					h = h[len(h)-n:]
				}
				var b strings.Builder
				for _, msg := range h {
					fmt.Fprintf(&b, "#%d %s: %s\n", msg.Seq, msg.From, msg.line())
				}
				if b.Len() == 0 {
					b.WriteString("(nothing on this channel yet)")
				}
				reply(m.ID, text(b.String()))

			case "collab_changes":
				n := p.Args.Count
				if n <= 0 {
					n = 20
				}
				var ch []Msg
				for _, msg := range fetch(channel(), 0) {
					if msg.kind() == KindChange {
						ch = append(ch, msg)
					}
				}
				if len(ch) > n {
					ch = ch[len(ch)-n:]
				}
				slices.Reverse(ch) // newest first, like git log
				var b strings.Builder
				var who string
				for _, msg := range ch {
					if msg.From != who {
						who = msg.From
						at := msg.At
						if len(at) >= 16 {
							at = strings.Replace(at[:16], "T", " ", 1)
						}
						fmt.Fprintf(&b, "\n%s — %s\n", who, at)
					}
					fmt.Fprintf(&b, "  %-8s %s — %s\n", msg.Action, msg.Target, msg.Text)
				}
				if b.Len() == 0 {
					b.WriteString("(no changes recorded yet)")
				}
				reply(m.ID, text(strings.TrimLeft(b.String(), "\n")))

			default:
				reply(m.ID, text("unknown tool "+p.Name))
			}

		case "resources/list":
			reply(m.ID, map[string]any{"resources": []any{}})
		case "prompts/list":
			reply(m.ID, map[string]any{"prompts": []any{}})
		case "ping":
			reply(m.ID, map[string]any{})
		default:
			if m.ID != nil {
				out.Encode(rpc{JSONRPC: "2.0", ID: m.ID, Error: &rpcErr{Code: -32601, Message: "no method " + m.Method}})
			}
		}
	}
}
