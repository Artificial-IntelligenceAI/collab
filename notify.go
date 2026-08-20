// Real OS notifications — the kind WhatsApp raises, with collab's own name and
// icon on them.
//
// Neither platform will let a plain Go binary do this. macOS attributes a
// notification to an application bundle, so the Mac side is a small signed
// Swift .app (notify/mac) that this shells out to. Windows attributes a toast
// to a registered AppUserModelID, so the Windows side is a small C# helper
// (notify/windows). Both are found next to this binary, and if neither is
// there collab simply stays quiet rather than falling back to something that
// pops up under another app's name.
package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// findNotifier returns the helper to run, or "" if this machine has none.
func findNotifier() string {
	var candidates []string
	exe, err := os.Executable()
	if err == nil {
		if p, err := filepath.EvalSymlinks(exe); err == nil {
			exe = p
		}
	}
	dir := filepath.Dir(exe)

	switch runtime.GOOS {
	case "darwin":
		for _, base := range []string{dir, home("Applications"), "/Applications"} {
			candidates = append(candidates, filepath.Join(base, "collab.app", "Contents", "MacOS", "collab-notify"))
		}
	case "windows":
		candidates = append(candidates,
			filepath.Join(dir, "collab-notify.exe"),
			filepath.Join(dir, "notify", "collab-notify.exe"))
	}
	for _, c := range candidates {
		if st, err := os.Stat(c); err == nil && !st.IsDir() {
			return c
		}
	}
	return ""
}

func notifyEnabled() bool { return env("COLLAB_NOTIFY", "1") != "0" }

// raise posts one notification. Failures are reported once and then ignored —
// a broken notifier must not take the watcher down with it.
var notifyWarned bool

func raise(helper, title, subtitle, body string) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, helper, title, body, subtitle).CombinedOutput()
	if err != nil && !notifyWarned {
		notifyWarned = true
		fmt.Fprintf(os.Stderr, "* notifications are not working (%v) %s\n", err, strings.TrimSpace(string(out)))
	}
}

func ellipsis(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	return strings.TrimSpace(string(r[:n-1])) + "…"
}

// ─────────────────────── coalescing ───────────────────────
//
// Messages are not popped one at a time. When a machine wakes after being
// asleep the server replays everything it missed at once, and forty popups in
// a row is not a notification, it is a punishment. So arrivals are collected
// until the channel goes quiet, and a burst becomes one summary.

const notifyQuiet = 700 * time.Millisecond

type notifier struct {
	in     chan Msg
	helper string
	me     string
}

func newNotifier(me string) *notifier {
	helper := findNotifier()
	if helper == "" || !notifyEnabled() {
		return nil
	}
	n := &notifier{in: make(chan Msg, 512), helper: helper, me: me}
	go n.run()
	return n
}

func (n *notifier) send(m Msg) {
	if n == nil {
		return
	}
	select {
	case n.in <- m:
	default: // never block the watcher for the sake of a popup
	}
}

func (n *notifier) run() {
	var pending []Msg
	var timer *time.Timer
	var fire <-chan time.Time
	for {
		select {
		case m := <-n.in:
			if strings.EqualFold(strings.TrimSpace(m.From), strings.TrimSpace(n.me)) {
				continue // your own words do not need announcing back to you
			}
			pending = append(pending, m)
			if timer == nil {
				timer = time.NewTimer(notifyQuiet)
			} else {
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
				timer.Reset(notifyQuiet)
			}
			fire = timer.C

		case <-fire:
			n.flush(pending)
			pending, fire = nil, nil
		}
	}
}

func (n *notifier) flush(ms []Msg) {
	switch len(ms) {
	case 0:
		return
	case 1:
		m := ms[0]
		if m.kind() == KindChange {
			raise(n.helper, m.From, fmt.Sprintf("%s · %s", m.Action, m.Target), ellipsis(m.Text, 180))
			return
		}
		raise(n.helper, m.From, "#"+m.Channel, ellipsis(m.Text, 180))
		return
	}

	var senders []string
	seen := map[string]bool{}
	changes := 0
	for _, m := range ms {
		if !seen[m.From] {
			seen[m.From] = true
			senders = append(senders, m.From)
		}
		if m.kind() == KindChange {
			changes++
		}
	}

	title := strings.Join(senders, " & ")
	if len(senders) > 2 {
		title = "collab"
	}
	sub := fmt.Sprintf("%d new on #%s", len(ms), ms[len(ms)-1].Channel)
	if changes > 0 {
		sub = fmt.Sprintf("%d new on #%s · %d change%s", len(ms), ms[len(ms)-1].Channel, changes, plural(changes))
	}
	last := ms[len(ms)-1]
	raise(n.helper, title, sub, ellipsis(last.From+": "+last.line(), 180))
}

func plural(n int) string {
	if n == 1 {
		return ""
	}
	return "s"
}

// testNotify is `collab test-notify` — the way to check the popups work
// without waiting for the other person to say something.
func testNotify() {
	h := findNotifier()
	if h == "" {
		fmt.Fprintf(os.Stderr, "collab: no notifier installed for %s\n", runtime.GOOS)
		switch runtime.GOOS {
		case "darwin":
			fmt.Fprintln(os.Stderr, "        put collab.app next to the collab binary, or in ~/Applications")
		case "windows":
			fmt.Fprintln(os.Stderr, "        put collab-notify.exe next to collab.exe")
		}
		os.Exit(1)
	}
	fmt.Printf("using %s\n", h)
	raise(h, "collab", "#"+channel()+" · test", "If you can see this, notifications work.")
	if !notifyWarned {
		fmt.Println("sent — it should be on screen now (and in Notification Centre)")
	}
}
