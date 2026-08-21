// Everything the window knows comes from collab.exe. The app never speaks the
// wire protocol itself: it runs `collab watch -json` and reads the stream, the
// same arrangement the Mac app uses. One implementation of the protocol, in
// Rust, and two windows that read its output.
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows;

namespace Collab
{
    public class Msg
    {
        public long Seq { get; set; }
        public string Channel { get; set; } = "";
        public string From { get; set; } = "";
        public string Host { get; set; } = "";
        public string Kind { get; set; } = "chat";
        public string Via { get; set; } = "";
        public string Text { get; set; } = "";
        public string Action { get; set; } = "";
        public string Target { get; set; } = "";
        public string At { get; set; } = "";
        public string FileName { get; set; } = "";
        public long FileSize { get; set; }
        public bool Replayed { get; set; }

        public bool IsChange => Kind == "change";
        public bool IsFile => !string.IsNullOrEmpty(FileName);
        public bool IsAI => Via == "ai";

        /// Who said it, as a person reads it. An AI is named for itself and the
        /// machine it sits on, because "which of us was that" has to stay
        /// answerable.
        public string Who => string.IsNullOrEmpty(From) ? Host : From;

        public string HHMM
        {
            get
            {
                return DateTimeOffset.TryParse(At, out var t)
                    ? t.ToLocalTime().ToString("HH:mm")
                    : "";
            }
        }

        public string Line => IsChange
            ? $"[{Action}] {Target} — {Text}"
            : IsFile
                ? $"[file] {FileName} ({Human(FileSize)}){(string.IsNullOrWhiteSpace(Text) ? "" : " — " + Text)}"
                : Text;

        public bool IsChat => !IsChange && !IsFile;
        public string RoleLabel => IsAI ? "AI" : "Human";
        /// An AI names itself and the machine it runs on; a person is just the
        /// machine. "Which of us was that" has to stay answerable.
        public string MachineLabel => IsAI && !string.IsNullOrEmpty(Host) && Host != Who ? Host : "";
        public bool ShowMachine => MachineLabel.Length > 0;
        public string ActionUpper => (Action ?? "").ToUpperInvariant();
        public string SizeLabel => Human(FileSize);
        public System.Windows.Media.Brush RoleBrush => IsAI ? Sol.ForName(Who) : Sol.FgDim;
        public System.Windows.Media.Brush ActionBrush => Sol.ForAction(Action);
        public System.Windows.Media.Brush AccentFg => Sol.OnAccent;
        public System.Windows.Media.Brush CyanBrush => Sol.Cyan;
        public System.Windows.Media.Brush EmBrush => Sol.FgEm;
        public System.Windows.Media.Brush CardBrush => Sol.BgAlt;
        public System.Windows.Media.Brush RuleBrush => Sol.Rule;
        public DateTimeOffset When =>
            DateTimeOffset.TryParse(At, out var t) ? t.ToLocalTime() : DateTimeOffset.MinValue;

        // The row template reads these. Keeping them on the message means the
        // list rebuilds in the right colours when the theme changes, without a
        // converter per column.
        public System.Windows.Media.Brush NameBrush =>
            IsChange ? Sol.ForAction(Action) : Sol.ForName(Who);
        public System.Windows.Media.Brush TextBrush => IsChange ? Sol.FgEm : Sol.Fg;
        public System.Windows.Media.Brush DimBrush => Sol.FgDim;

        static string Human(long n) =>
            n >= 1048576 ? $"{n / 1048576.0:0.#} MB" :
            n >= 1024    ? $"{n / 1024.0:0.#} KB" : $"{n} B";
    }

    /// A break in the stream, so a conversation that spans days reads as one.
    public class DaySep
    {
        public string Label { get; set; } = "";
        public System.Windows.Media.Brush RuleBrush => Sol.Rule;
        public System.Windows.Media.Brush DimBrush => Sol.FgDim;

        public static string LabelFor(DateTimeOffset d)
        {
            var day = d.Date;
            if (day == DateTime.Today) return "TODAY";
            if (day == DateTime.Today.AddDays(-1)) return "YESTERDAY";
            return d.ToString("ddd d MMM").ToUpperInvariant();
        }
    }

    public class Core
    {
        public ObservableCollection<Msg> Messages { get; } = new();
        public ObservableCollection<string> Channels { get; } = new();
        public string Me { get; private set; } = "";
        public string ServerAddr { get; private set; } = "";
        public bool Connected { get; private set; }
        public string? Fatal { get; private set; }

        public event Action? Changed;
        /// Raised for messages that arrive live — never for backlog. A toast
        /// for something said two hours ago claims it was just said.
        public event Action<Msg>? Arrived;

        Process? watcher;

        /// collab.exe sits next to this app; falling back to PATH keeps a
        /// developer's copy working.
        public static string Exe
        {
            get
            {
                // Windows filesystems are case-insensitive, so the app and the
                // command line cannot both be called collab.exe in one folder —
                // copying one over the other silently destroys it, which is
                // exactly what happened the first time this was deployed. The
                // command line therefore lives in bin\, the way the Mac keeps
                // it inside Collab.app.
                var dir = AppContext.BaseDirectory;
                foreach (var p in new[]
                {
                    Path.Combine(dir, "bin", "collab.exe"),
                    Path.Combine(dir, "collab.exe"),
                })
                    if (File.Exists(p)) return p;
                return "collab.exe";
            }
        }

        public static string Run(string args)
        {
            try
            {
                var psi = new ProcessStartInfo(Exe, args)
                {
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true,
                };
                using var p = Process.Start(psi);
                if (p == null) return "";
                var outp = p.StandardOutput.ReadToEnd();
                var err = p.StandardError.ReadToEnd();
                p.WaitForExit();
                return p.ExitCode == 0 ? outp : (string.IsNullOrWhiteSpace(err) ? outp : err);
            }
            catch (Exception e) { return "collab: " + e.Message; }
        }

        public void LoadWho()
        {
            try
            {
                var j = JsonDocument.Parse(Run("who -json")).RootElement;
                Me = j.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                ServerAddr = j.TryGetProperty("addr", out var a) ? a.GetString() ?? "" : "";
                Channels.Clear();
                if (j.TryGetProperty("channels", out var cs))
                    foreach (var c in cs.EnumerateArray())
                        Channels.Add(c.GetString() ?? "");
            }
            catch (Exception e) { Fatal = "cannot read collab's settings — " + e.Message; }
            Changed?.Invoke();
        }

        public void Start()
        {
            LoadWho();
            var psi = new ProcessStartInfo(Exe, "watch -json -all -since 0 -no-save")
            {
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            try { watcher = Process.Start(psi); }
            catch (Exception e) { Fatal = "cannot start collab.exe — " + e.Message; Changed?.Invoke(); return; }
            if (watcher == null) { Fatal = "cannot start collab.exe"; Changed?.Invoke(); return; }
            Task.Run(() => Pump(watcher.StandardOutput));
        }

        void Pump(StreamReader r)
        {
            string? line;
            while ((line = r.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] != '{') continue;
                Msg? live = null;
                try
                {
                    var d = JsonDocument.Parse(line).RootElement;
                    var type = d.TryGetProperty("type", out var t) ? t.GetString() : null;
                    if (type == "status")
                    {
                        Connected = d.TryGetProperty("connected", out var c) && c.GetBoolean();
                        Application.Current?.Dispatcher.Invoke(() => Changed?.Invoke());
                        continue;
                    }
                    if (type != "msg" || !d.TryGetProperty("msg", out var m)) continue;
                    var msg = Parse(m);
                    msg.Replayed = d.TryGetProperty("replayed", out var rp) && rp.GetBoolean();
                    live = msg;
                }
                catch { continue; }
                if (live == null) continue;
                Application.Current?.Dispatcher.Invoke(() =>
                {
                    Messages.Add(live);
                    Changed?.Invoke();
                    // Backlog is history. Only what arrives live is news.
                    if (!live.Replayed) Arrived?.Invoke(live);
                });
            }
        }

        static Msg Parse(JsonElement m)
        {
            string S(string k) => m.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String
                ? v.GetString() ?? "" : "";
            var msg = new Msg
            {
                Seq = m.TryGetProperty("seq", out var s) ? s.GetInt64() : 0,
                Channel = S("channel"), From = S("from"), Host = S("host"),
                Kind = m.TryGetProperty("kind", out var k) && k.ValueKind == JsonValueKind.String
                    ? k.GetString() ?? "chat" : "chat",
                Via = S("via"), Text = S("text"), Action = S("action"),
                Target = S("target"), At = S("at"),
            };
            if (m.TryGetProperty("file", out var f) && f.ValueKind == JsonValueKind.Object)
            {
                msg.FileName = f.TryGetProperty("name", out var fn) ? fn.GetString() ?? "" : "";
                msg.FileSize = f.TryGetProperty("size", out var fs) ? fs.GetInt64() : 0;
            }
            return msg;
        }

        /// Quoting for a command line, so a message containing a quote mark is
        /// sent rather than mangled.
        static string Q(string s) => "\"" + s.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";

        public string Post(string channel, string text) =>
            Run($"post -c {Q(channel)} {Q(text)}");

        public string SendFile(string channel, string path, string caption) =>
            Run($"send {Q(path)} -c {Q(channel)} -m {Q(caption)}");

        /// Names only. The keys are deliberately not asked for here — this list
        /// is for the picker, and a picker has no business holding secrets.
        public System.Collections.Generic.List<string> ChannelNames()
        {
            var outp = new System.Collections.Generic.List<string>();
            try
            {
                foreach (var c in JsonDocument.Parse(Run("channels -json")).RootElement.EnumerateArray())
                    if (c.TryGetProperty("name", out var n)) outp.Add(n.GetString() ?? "");
            }
            catch { }
            return outp;
        }

        public void Stop()
        {
            try { if (watcher is { HasExited: false }) watcher.Kill(true); } catch { }
            watcher = null;
        }
    }
}
