// The MCP side. We proved a server cannot push anything into a session, so this
// deliberately offers only tools — pulling. The push still comes from `collab
// watch` running under a Monitor.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
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
				"serverInfo":   map[string]string{"name": "collab", "version": "1.0.0"},
			})

		case "tools/list":
			reply(m.ID, map[string]any{"tools": []any{
				map[string]any{
					"name":        "collab_post",
					"description": "Send a message to the other person's Claude on the shared channel. Use this to say what you just changed, what you are about to touch, or to ask them something.",
					"inputSchema": map[string]any{
						"type": "object",
						"properties": map[string]any{
							"message": map[string]any{"type": "string", "description": "What to tell them."},
						},
						"required": []string{"message"},
					},
				},
				map[string]any{
					"name":        "collab_recent",
					"description": "Read the most recent messages on the shared channel, oldest first.",
					"inputSchema": map[string]any{
						"type": "object",
						"properties": map[string]any{
							"count": map[string]any{"type": "integer", "description": "How many messages (default 20)."},
						},
					},
				},
			}})

		case "tools/call":
			var p struct {
				Name string `json:"name"`
				Args struct {
					Message string `json:"message"`
					Count   int    `json:"count"`
				} `json:"arguments"`
			}
			json.Unmarshal(m.Params, &p)
			switch p.Name {
			case "collab_post":
				if strings.TrimSpace(p.Args.Message) == "" {
					reply(m.ID, text("nothing to send — message was empty"))
					break
				}
				post(p.Args.Message)
				reply(m.ID, text("sent: "+p.Args.Message))
			case "collab_recent":
				n := p.Args.Count
				if n <= 0 {
					n = 20
				}
				h := readHistory()
				if len(h) > n {
					h = h[len(h)-n:]
				}
				var b strings.Builder
				for _, msg := range h {
					fmt.Fprintf(&b, "#%d %s: %s\n", msg.Seq, msg.From, msg.Text)
				}
				if b.Len() == 0 {
					b.WriteString("(no messages yet)")
				}
				reply(m.ID, text(b.String()))
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
