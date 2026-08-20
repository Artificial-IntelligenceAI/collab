// The window. A local web page served by this same binary — no libraries, so the
// Windows .exe is still one build command away.
//
// It watches the channel the same way `collab watch` does, with the same resume
// rule, and keeps its own place in the sequence so it never fights with the
// Monitor's watcher over ~/.collab-seen.
package main

import (
	"embed"
	"encoding/json"
	"flag"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"runtime"
	"sync"
	"time"
)

//go:embed ui.html
var uiFS embed.FS

type gui struct {
	mu    sync.Mutex
	msgs  []Msg
	last  int64
	up    bool
	subs  map[chan string]bool
	popup *notifier
}

func (g *gui) since() int64 {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.last
}

func (g *gui) add(m Msg) {
	g.mu.Lock()
	if m.Seq <= g.last {
		g.mu.Unlock()
		return // already have it; resuming must not duplicate
	}
	g.msgs = append(g.msgs, m)
	g.last = m.Seq
	g.mu.Unlock()
	g.popup.send(m)
	b, _ := json.Marshal(m)
	g.emit("msg", string(b))
}

func (g *gui) status(up bool, from int64) {
	g.mu.Lock()
	changed := g.up != up
	g.up = up
	g.mu.Unlock()
	if !changed {
		return
	}
	b, _ := json.Marshal(map[string]any{"connected": up, "from": from, "addr": addr()})
	g.emit("status", string(b))
}

func (g *gui) emit(event, data string) {
	g.mu.Lock()
	subs := make([]chan string, 0, len(g.subs))
	for s := range g.subs {
		subs = append(subs, s)
	}
	g.mu.Unlock()
	frame := fmt.Sprintf("event: %s\ndata: %s\n\n", event, data)
	for _, s := range subs {
		select {
		case s <- frame:
		default:
		}
	}
}

func runGUI(args []string) {
	fs := flag.NewFlagSet("gui", flag.ContinueOnError)
	fs.SetOutput(os.Stderr)
	noOpen := fs.Bool("no-open", false, "don't open a browser, just print the address")
	popups := fs.Bool("notify", false, "raise OS notifications too (`collab watch` already does this)")
	// A window opened by clicking a notification is not started from your
	// shell, so it inherits none of your COLLAB_ settings. The click passes
	// them in instead, and they land in the environment so that posting from
	// the window goes to the same place `collab post` would.
	wantChan := fs.String("channel", "", "which channel this window is for (default $COLLAB_CHANNEL)")
	wantName := fs.String("name", "", "who to post as (default $COLLAB_NAME)")
	uri := fs.String("uri", "", "a collab://open?channel=… URL, as handed over by a Windows toast click")
	if fs.Parse(args) != nil {
		os.Exit(2)
	}
	if *uri != "" {
		if u, err := url.Parse(*uri); err == nil {
			if v := u.Query().Get("channel"); v != "" && *wantChan == "" {
				*wantChan = v
			}
			if v := u.Query().Get("name"); v != "" && *wantName == "" {
				*wantName = v
			}
		}
	}
	if *wantChan != "" {
		os.Setenv("COLLAB_CHANNEL", *wantChan)
	}
	if *wantName != "" {
		os.Setenv("COLLAB_NAME", *wantName)
	}

	g := &gui{subs: map[chan string]bool{}}
	if *popups {
		g.popup = newNotifier(name())
	}

	// Watch every channel — the views filter, the wire does not.
	go stream("", g.since, g.add, g.status)

	mux := http.NewServeMux()

	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		b, err := uiFS.ReadFile("ui.html")
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Write(b)
	})

	mux.HandleFunc("/api/state", func(w http.ResponseWriter, r *http.Request) {
		g.mu.Lock()
		msgs := append([]Msg(nil), g.msgs...)
		st := map[string]any{"connected": g.up, "last": g.last, "me": name(), "channel": channel(), "addr": addr(), "messages": msgs}
		g.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(st)
	})

	mux.HandleFunc("/api/post", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			http.Error(w, "POST only", 405)
			return
		}
		var body struct {
			Text    string `json:"text"`
			Channel string `json:"channel"`
		}
		json.NewDecoder(r.Body).Decode(&body)
		ch := body.Channel
		if ch == "" {
			ch = channel()
		}
		if err := sendTo(ch, Msg{Kind: KindChat, Text: body.Text}); err != nil {
			http.Error(w, err.Error(), 502)
			return
		}
		w.Write([]byte(`{"ok":true}`))
	})

	mux.HandleFunc("/api/stream", func(w http.ResponseWriter, r *http.Request) {
		fl, ok := w.(http.Flusher)
		if !ok {
			http.Error(w, "no streaming", 500)
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		w.Header().Set("Cache-Control", "no-cache")
		w.Header().Set("Connection", "keep-alive")

		ch := make(chan string, 256)
		g.mu.Lock()
		g.subs[ch] = true
		up, last := g.up, g.last
		g.mu.Unlock()
		defer func() { g.mu.Lock(); delete(g.subs, ch); g.mu.Unlock() }()

		b, _ := json.Marshal(map[string]any{"connected": up, "from": last, "addr": addr()})
		fmt.Fprintf(w, "event: status\ndata: %s\n\n", b)
		fl.Flush()

		tick := time.NewTicker(20 * time.Second)
		defer tick.Stop()
		for {
			select {
			case frame := <-ch:
				fmt.Fprint(w, frame)
				fl.Flush()
			case <-tick.C:
				fmt.Fprint(w, ": ping\n\n") // keep the pipe warm
				fl.Flush()
			case <-r.Context().Done():
				return
			}
		}
	})

	port := env("COLLAB_GUI_PORT", "8788")
	target := guiURL()
	// 127.0.0.1 only: this window is yours, not the network's.
	ln, err := net.Listen("tcp", "127.0.0.1:"+port)
	if err != nil {
		// Asking for the window twice should show you the window, not an
		// error — clicking a notification does exactly that.
		if guiIsRunning(target) {
			fmt.Printf("collab window already open: %s\n", target)
			if !*noOpen {
				openBrowser(withChannel(target, channel()))
			}
			return
		}
		fmt.Fprintf(os.Stderr, "collab: cannot open the window on port %s — %v\n", port, err)
		fmt.Fprintln(os.Stderr, "        (something else is using it? try COLLAB_GUI_PORT=8789 collab gui)")
		os.Exit(1)
	}
	fmt.Printf("collab window: %s\n", target)
	fmt.Printf("watching %s  (Ctrl-C to close)\n", addr())
	if !*noOpen {
		openBrowser(withChannel(target, channel()))
	}
	http.Serve(ln, mux)
}

func guiURL() string { return "http://127.0.0.1:" + env("COLLAB_GUI_PORT", "8788") }

// withChannel asks the window to open showing a particular channel — a click on
// a notification should land where the message came from, not on the default.
func withChannel(base, ch string) string {
	if ch == "" {
		return base
	}
	return base + "/?channel=" + url.QueryEscape(ch)
}

// guiIsRunning reports whether the thing holding the port is our own window.
func guiIsRunning(url string) bool {
	c := &http.Client{Timeout: 2 * time.Second}
	r, err := c.Get(url + "/api/state")
	if err != nil {
		return false
	}
	defer r.Body.Close()
	var st struct {
		Channel *string `json:"channel"`
	}
	return r.StatusCode == 200 && json.NewDecoder(r.Body).Decode(&st) == nil && st.Channel != nil
}

func openBrowser(url string) {
	var c *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		c = exec.Command("open", url)
	case "windows":
		c = exec.Command("rundll32", "url.dll,FileProtocolHandler", url)
	default:
		c = exec.Command("xdg-open", url)
	}
	c.Start()
}
