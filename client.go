// The client side: watch, post, change, log.
package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"net"
	"os"
	"slices"
	"strconv"
	"strings"
	"time"
)

func lastSeen() int64 {
	b, err := os.ReadFile(seenPath)
	if err != nil {
		return 0
	}
	n, _ := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64)
	return n
}

// Losing our place is not allowed to be quiet either: if we cannot write it down
// we would replay the whole history on the next reconnect and never say why.
var seenWarned bool

func saveSeen(n int64) {
	if err := os.WriteFile(seenPath, []byte(strconv.FormatInt(n, 10)), 0o644); err != nil && !seenWarned {
		seenWarned = true
		fmt.Fprintf(os.Stderr, "* cannot record my place in %s (%v) — after a reconnect you may see old messages again\n", seenPath, err)
	}
}

// stream dials the server and delivers messages to onMsg until the connection
// dies, then reconnects — announcing both, because silence must always mean
// "nobody is talking" and never "the wire died". Resumes from `since()`.
func stream(ch string, since func() int64, onMsg func(Msg), onStatus func(up bool, resumeFrom int64)) {
	announced := false
	for {
		c, err := net.Dial("tcp", addr())
		if err != nil {
			if !announced {
				onStatus(false, since())
				announced = true
			}
			time.Sleep(2 * time.Second)
			continue
		}
		announced = false
		onStatus(true, since())
		json.NewEncoder(c).Encode(Hello{Name: name(), Channel: ch, Since: since(), Mode: "watch"})

		sc := bufio.NewScanner(c)
		sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
		for sc.Scan() {
			var m Msg
			if json.Unmarshal(sc.Bytes(), &m) != nil {
				continue
			}
			onMsg(m)
		}
		c.Close()
		if !announced {
			onStatus(false, since())
			announced = true
		}
		time.Sleep(2 * time.Second)
	}
}

func watch() {
	first := true
	popup := newNotifier(name()) // nil when this machine has no notifier
	stream(channel(), lastSeen,
		func(m Msg) {
			fmt.Printf("[%s] %s: %s\n", m.Channel, m.From, m.line())
			saveSeen(m.Seq)
			popup.send(m)
		},
		func(up bool, from int64) {
			if up {
				if !first { // the very first connect is not a "re"connect
					fmt.Printf("* reconnected to %s, resuming from #%d\n", addr(), from)
				}
				first = false
				return
			}
			first = false
			fmt.Printf("* DISCONNECTED from %s — retrying\n", addr())
		})
}

// send delivers one message. The server stamps seq, from, at and channel.
func send(m Msg) error {
	c, err := net.Dial("tcp", addr())
	if err != nil {
		return err
	}
	defer c.Close()
	enc := json.NewEncoder(c)
	if err := enc.Encode(Hello{Name: name(), Channel: channel(), Mode: "post"}); err != nil {
		return err
	}
	if err := enc.Encode(m); err != nil {
		return err
	}
	time.Sleep(200 * time.Millisecond) // let it land before we hang up
	return nil
}

func post(text string) {
	text = strings.ReplaceAll(strings.TrimSpace(text), "\n", " ")
	if text == "" {
		fmt.Fprintln(os.Stderr, `usage: collab post "message"`)
		os.Exit(2)
	}
	if err := send(Msg{Kind: KindChat, Text: text}); err != nil {
		fmt.Fprintf(os.Stderr, "collab: cannot reach %s — %v\n", addr(), err)
		os.Exit(1)
	}
}

const changeUsage = `usage: collab change -action added|edited|removed|renamed -target "where" "one-line summary"

  -action   what you did to it
  -target   which script or instance, e.g. ServerScriptService/ShopHandler
  summary   one line, in past tense: "gave the buy button a debounce"`

func changeCmd(args []string) {
	fs := flag.NewFlagSet("change", flag.ContinueOnError)
	fs.SetOutput(os.Stderr)
	action := fs.String("action", "", "added | edited | removed | renamed")
	target := fs.String("target", "", "which script or instance")
	fs.Usage = func() { fmt.Fprintln(os.Stderr, changeUsage) }
	if fs.Parse(args) != nil {
		os.Exit(2)
	}
	summary := strings.ReplaceAll(strings.TrimSpace(strings.Join(fs.Args(), " ")), "\n", " ")

	*action = strings.ToLower(strings.TrimSpace(*action))
	if !slices.Contains(actions, *action) {
		fmt.Fprintf(os.Stderr, "collab: -action must be one of %s\n\n%s\n", strings.Join(actions, ", "), changeUsage)
		os.Exit(2)
	}
	if strings.TrimSpace(*target) == "" || summary == "" {
		fmt.Fprintln(os.Stderr, changeUsage)
		os.Exit(2)
	}
	if err := send(Msg{Kind: KindChange, Action: *action, Target: strings.TrimSpace(*target), Text: summary}); err != nil {
		fmt.Fprintf(os.Stderr, "collab: cannot reach %s — %v\n", addr(), err)
		os.Exit(1)
	}
}

// fetch asks the server for history. The server owns the only complete copy, so
// the machine that isn't the server has to ask over the wire; if it can't, it
// falls back to whatever it has locally rather than claiming the channel is empty.
func fetch(ch string, since int64) []Msg {
	c, err := net.Dial("tcp", addr())
	if err != nil {
		return filterHistory(readHistory(), ch, since)
	}
	defer c.Close()
	c.SetDeadline(time.Now().Add(10 * time.Second))
	if json.NewEncoder(c).Encode(Hello{Name: name(), Channel: ch, Since: since, Mode: "fetch"}) != nil {
		return filterHistory(readHistory(), ch, since)
	}
	var out []Msg
	sc := bufio.NewScanner(c)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for sc.Scan() {
		var m Msg
		if json.Unmarshal(sc.Bytes(), &m) == nil {
			out = append(out, m)
		}
	}
	if out == nil {
		return filterHistory(readHistory(), ch, since)
	}
	return out
}

func showLog(args []string) {
	fs := flag.NewFlagSet("log", flag.ContinueOnError)
	fs.SetOutput(os.Stderr)
	only := fs.Bool("changes", false, "only recorded changes")
	all := fs.Bool("all", false, "every channel, not just $COLLAB_CHANNEL")
	if fs.Parse(args) != nil {
		os.Exit(2)
	}
	ch := channel()
	if *all {
		ch = ""
	}
	for _, m := range fetch(ch, 0) {
		if *only && m.kind() != KindChange {
			continue
		}
		at := m.At
		if len(at) >= 16 {
			at = at[11:16]
		}
		fmt.Printf("#%-4d [%s] %s: %s\n", m.Seq, at, m.From, m.line())
	}
}
