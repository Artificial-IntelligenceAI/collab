using System;
using System.Threading.Tasks;
using System.Collections.Generic;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace Collab
{
    public partial class MainWindow : Window
    {
        readonly Core core = new();
        string view = "chat";
        string channel = "";
        List<string> names = new();     // who can be @mentioned here
        static string meChannel = "";   // channel EmojiText.Me was computed for

        /// Call after anything that changes what this machine is called on a
        /// channel. Without it a rename leaves the cache holding the old name,
        /// and your own mentions stop standing out until you switch channels
        /// and back — a staleness with no symptom except the thing quietly not
        /// working.
        public static void ForgetMe() => meChannel = "";

        public MainWindow()
        {
            InitializeComponent();
            Loaded += OnLoaded;
            Closed += (_, _) => core.Stop();
            Activated += (_, _) =>
            {
                var dark = Sol.SystemPrefersDark();
                if (dark != Sol.Dark) { Sol.Dark = dark; Paint(); Rebuild(); }
            };
        }

        void OnLoaded(object? s, RoutedEventArgs e)
        {
            Paint();
            core.Changed += () => Dispatcher.Invoke(Sync);
            core.Arrived += OnArrived;
            core.Start();
            Entry.Focus();
        }

        void Paint()
        {
            Background = Sol.Bg;
            Root.Background = Sol.Bg;
            ServerLabel.Foreground = Sol.FgDim;
            EmptyNote.Foreground = Sol.FgDim;
            ComposerBar.Background = Sol.BgAlt;
            ComposerBar.BorderBrush = Sol.Rule;
            ComposerBar.BorderThickness = new Thickness(0, 1, 0, 0);
            foreach (var t in new[] { Entry, Search })
            {
                t.Background = Sol.Bg;
                t.Foreground = Sol.FgEm;
                t.BorderBrush = Sol.Rule;
                t.CaretBrush = Sol.FgEm;
            }
            SendBtn.Background = Sol.Blue; SendBtn.Foreground = Sol.OnAccent; SendBtn.BorderBrush = Sol.Blue;
            foreach (var b in new[] { AttachBtn, ChannelsBtn, UpdateBtn })
            {
                b.Background = Sol.BgAlt; b.Foreground = Sol.Fg; b.BorderBrush = Sol.Rule;
            }
            ChannelPicker.Background = Sol.BgAlt; ChannelPicker.Foreground = Sol.FgEm;
            Suggest.Background = Sol.Bg; Suggest.Foreground = Sol.FgEm; Suggest.BorderBrush = Sol.Rule;
            PaintTabs();
            Footer.Foreground = Sol.FgDim;
        }

        void PaintTabs()
        {
            foreach (var (b, name) in new[] { (TabChat, "chat"), (TabChanges, "changes") })
            {
                var on = view == name;
                b.Background = on ? Sol.Blue : Sol.BgAlt;
                b.Foreground = on ? Sol.OnAccent : Sol.Fg;
                b.BorderBrush = on ? Sol.Blue : Sol.Rule;
                b.FontWeight = on ? FontWeights.SemiBold : FontWeights.Normal;
            }
        }

        void Sync()
        {
            // The light is about the channel on screen. Another channel being
            // down is worth saying and is not this room's state.
            core.Watching = channel;
            var others = core.OthersDown;
            Dot.Fill = core.Connected ? (others.Count == 0 ? Sol.Green : Sol.Yellow) : Sol.Red;
            ServerLabel.Text = core.Fatal ?? (core.Connected
                ? (others.Count == 0
                    ? core.ServerAddr
                    : others.Count == 1
                        ? core.ServerAddr + "  \u00b7  #" + others[0] + " down"
                        : core.ServerAddr + "  \u00b7  " + others.Count + " channels down")
                : "DISCONNECTED from " + core.ServerAddr + " on #" + channel + " — retrying");
            ServerLabel.ToolTip = others.Count == 0
                ? null
                : "#" + channel + " is connected. Not connected on " + string.Join(", ", others.Select(c => "#" + c));
            if (ChannelPicker.Items.Count != core.Channels.Count)
            {
                var keep = channel;
                ChannelPicker.Items.Clear();
                foreach (var c in core.Channels) ChannelPicker.Items.Add(c);
                channel = core.Channels.Contains(keep) ? keep : core.Channels.FirstOrDefault() ?? "";
                ChannelPicker.SelectedItem = channel;
            }
            Rebuild();
        }

        void Rebuild()
        {
            if (string.IsNullOrEmpty(channel)) return;
            var q = Search.Text.Trim();
            var msgs = core.Messages
                .Where(m => m.Channel == channel)
                .Where(m => view == "changes" ? m.IsChange : !m.IsChange)
                .Where(m => q.Length == 0
                            || m.Text.Contains(q, StringComparison.OrdinalIgnoreCase)
                            || m.Who.Contains(q, StringComparison.OrdinalIgnoreCase)
                            || m.Target.Contains(q, StringComparison.OrdinalIgnoreCase)
                            || m.FileName.Contains(q, StringComparison.OrdinalIgnoreCase))
                .ToList();

            // Who this window answers to on this channel, so a mention aimed at
            // it is drawn differently from one aimed at somebody else. The
            // display name wins where there is one, the same order the CLI uses.
            //
            // Cached per channel: both of those calls spawn the CLI, and this
            // runs on every redraw — which is every arriving message. The name
            // only changes when the channel does, or when somebody renames
            // themselves on it, and that path calls Reload with meCached
            // cleared.
            if (meChannel != channel)
            {
                EmojiText.Me = Addressable(Core.DisplayOn(channel) ?? Core.MachineName);
                meChannel = channel;
            }

            // Who can be addressed here, for the @ suggestions.
            names = core.Messages.Where(m => m.Channel == channel)
                .Select(m => Addressable(m.Who)).Where(n => n.Length > 0)
                .Distinct(StringComparer.OrdinalIgnoreCase).OrderBy(n => n).ToList();

            var rows = new List<object>();
            DateTime last = DateTime.MinValue;
            foreach (var m in msgs)
            {
                var day = m.When.Date;
                if (day != last && m.When != DateTimeOffset.MinValue)
                {
                    rows.Add(new DaySep { Label = DaySep.LabelFor(m.When) });
                    last = day;
                }
                rows.Add(m);
            }
            List.ItemsSource = rows;

            EmptyNote.Visibility = rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            EmptyNote.Text = q.Length > 0
                ? $"Nothing matching “{q}” on #{channel}."
                : view == "changes"
                    ? $"No changes recorded on #{channel} yet.\nThey appear here when either side records one."
                    : $"Nothing on #{channel} yet.\nSay something below and it reaches the other machine.";

            Entry.IsEnabled = SendBtn.IsEnabled = view == "chat";
            Footer.Text = $"posting as {core.DisplayName(channel)} on #{channel}";
            Footer.Foreground = Sol.FgDim;
            Scroller.ScrollToEnd();
        }

        void OnArrived(Msg m)
        {
            if (m.Who == core.Me && !m.IsAI) return;
            if (IsActive && m.Channel == channel) return;
            Toast.Post("collab", "#" + m.Channel + " · " + m.Who, m.Line);
        }

        // ── @ suggestions ──────────────────────────────────────

        /// The form a mention has to be written in, mirroring `addressable()` in
        /// core/src/msg.rs. A display name may hold spaces and capitals, and the
        /// parser stops a mention at the first space — so offering "Big Fable"
        /// verbatim autofills @Big, which addresses nobody. A suggestion that
        /// cannot work is worse than no suggestion.
        static string Addressable(string name) =>
            string.Join("-", (name ?? "").ToLowerInvariant()
                .Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));

        /// The word being typed after an @, if the caret is inside one.
        string? MentionPrefix()
        {
            var text = Entry.Text;
            var caret = Entry.CaretIndex;
            if (caret == 0 || caret > text.Length) return null;
            var at = text.LastIndexOf('@', Math.Max(0, caret - 1));
            if (at < 0) return null;
            if (at > 0 && (char.IsLetterOrDigit(text[at - 1]) || text[at - 1] == '@')) return null;
            var word = text.Substring(at + 1, caret - at - 1);
            return word.Any(char.IsWhiteSpace) ? null : word;
        }

        void OnDraft(object s, TextChangedEventArgs e)
        {
            var p = MentionPrefix();
            if (p == null) { Suggest.Visibility = Visibility.Collapsed; return; }
            var hits = names.Where(n => n.StartsWith(p, StringComparison.OrdinalIgnoreCase))
                            .Take(6).ToList();
            if (hits.Count == 0) { Suggest.Visibility = Visibility.Collapsed; return; }
            Suggest.ItemsSource = hits;
            Suggest.SelectedIndex = 0;
            Suggest.Visibility = Visibility.Visible;
        }

        void Accept(string name)
        {
            var text = Entry.Text;
            var caret = Entry.CaretIndex;
            var at = text.LastIndexOf('@', Math.Max(0, caret - 1));
            if (at < 0) return;
            Entry.Text = text.Substring(0, at + 1) + name + " " + text.Substring(caret);
            Entry.CaretIndex = at + 1 + name.Length + 1;
            Suggest.Visibility = Visibility.Collapsed;
        }

        void OnSuggestKey(object s, KeyEventArgs e) { }
        void OnSuggestPick(object s, MouseButtonEventArgs e)
        {
            if (Suggest.SelectedItem is string n) { Accept(n); Entry.Focus(); }
        }

        void OnKey(object s, KeyEventArgs e)
        {
            if (Suggest.Visibility == Visibility.Visible)
            {
                // While the list is open, Return takes the highlighted name
                // rather than sending — that is what makes it a suggestion
                // instead of an obstacle.
                switch (e.Key)
                {
                    case Key.Down:
                        Suggest.SelectedIndex = Math.Min(Suggest.SelectedIndex + 1, Suggest.Items.Count - 1);
                        e.Handled = true; return;
                    case Key.Up:
                        Suggest.SelectedIndex = Math.Max(Suggest.SelectedIndex - 1, 0);
                        e.Handled = true; return;
                    case Key.Enter:
                    case Key.Tab:
                        // Take the highlighted name, or the first one if the
                        // list has not settled its selection. Assigning
                        // ItemsSource resets selection, and an index set before
                        // the containers exist can be cleared — so the list is
                        // visible with nothing selected, and the old code
                        // swallowed Return in that state: it neither inserted a
                        // name nor sent the message. Nothing happened, which is
                        // the worst of the three.
                        //
                        // If there is genuinely nothing to take, fall through
                        // and let Return do what Return does.
                        var pick = Suggest.SelectedItem as string
                                   ?? (Suggest.Items.Count > 0 ? Suggest.Items[0] as string : null);
                        if (pick != null) { Accept(pick); e.Handled = true; return; }
                        Suggest.Visibility = Visibility.Collapsed;
                        break;
                    case Key.Escape:
                        Suggest.Visibility = Visibility.Collapsed; e.Handled = true; return;
                }
            }
            if (e.Key == Key.Enter && Keyboard.Modifiers == ModifierKeys.None)
            {
                OnSend(s, e); e.Handled = true;
            }
        }

        // ── actions ────────────────────────────────────────────

        /// A RichTextBox eats the mouse wheel even with its own scrollbars off,
        /// so the conversation would stop scrolling whenever the pointer was
        /// over a message — which is most of the time. Hand the event to the
        /// list instead.
        void OnRowWheel(object s, MouseWheelEventArgs e)
        {
            if (e.Handled) return;
            e.Handled = true;
            Scroller.RaiseEvent(new MouseWheelEventArgs(e.MouseDevice, e.Timestamp, e.Delta)
            {
                RoutedEvent = UIElement.MouseWheelEvent,
                Source = s,
            });
        }

        void OnChat(object s, RoutedEventArgs e) { view = "chat"; PaintTabs(); Rebuild(); }
        void OnChanges(object s, RoutedEventArgs e) { view = "changes"; PaintTabs(); Rebuild(); }
        void OnSearch(object s, TextChangedEventArgs e) => Rebuild();

        void OnChannel(object s, SelectionChangedEventArgs e)
        {
            if (ChannelPicker.SelectedItem is string c && c != channel) { channel = c; Rebuild(); }
        }

        void OnSend(object s, RoutedEventArgs e)
        {
            var text = Entry.Text.Trim();
            if (text.Length == 0 || string.IsNullOrEmpty(channel)) return;
            // Posts to the channel on screen, not the one it started on.
            var target = channel;
            Entry.Text = "";
            Suggest.Visibility = Visibility.Collapsed;
            // Off the UI thread. Posting shells out to the CLI, which dials the
            // server — and when the message carries an @, it also pulls the
            // channel's speaker list before sending. Measured from the VM that
            // is 2.3s for a plain message and 4.5s or more for a mention, and
            // every one of those seconds was a frozen window.
            //
            // The text is cleared immediately, so the send feels done while it
            // finishes; only a refusal comes back, and it comes back to the
            // footer where a refusal belongs.
            Footer.Text = "sending…";
            Footer.Foreground = Sol.FgDim;
            Task.Run(() => core.Post(target, text)).ContinueWith(t =>
            {
                var reply = t.IsFaulted ? "collab: " + t.Exception?.GetBaseException().Message
                                        : t.Result;
                if (reply.StartsWith("collab:") || reply.Contains("REFUSED"))
                {
                    Footer.Text = reply.Split('\n')[0];
                    Footer.Foreground = Sol.Red;
                }
                else
                {
                    Footer.Text = $"posting as {core.DisplayName(channel)} on #{channel}";
                    Footer.Foreground = Sol.FgDim;
                }
            }, TaskScheduler.FromCurrentSynchronizationContext());
        }

        void OnAttach(object s, RoutedEventArgs e)
        {
            var d = new Microsoft.Win32.OpenFileDialog { Title = "Send a file to #" + channel };
            if (d.ShowDialog() == true) SendFile(d.FileName);
        }

        void SendFile(string path)
        {
            var reply = core.SendFile(channel, path, "");
            Footer.Text = reply.Split('\n')[0].Trim();
            Footer.Foreground = reply.StartsWith("collab:") ? Sol.Red : Sol.Green;
        }

        void OnDragOver(object s, DragEventArgs e)
        {
            e.Effects = e.Data.GetDataPresent(DataFormats.FileDrop) ? DragDropEffects.Copy : DragDropEffects.None;
            e.Handled = true;
        }

        void OnDrop(object s, DragEventArgs e)
        {
            if (e.Data.GetData(DataFormats.FileDrop) is string[] files && files.Length > 0)
                SendFile(files[0]);
        }

        void OnSaveFile(object s, RoutedEventArgs e)
        {
            if (s is not Button b || b.Tag is not Msg m) return;
            var d = new Microsoft.Win32.SaveFileDialog { FileName = m.FileName };
            if (d.ShowDialog() != true) return;
            var reply = Core.Run($"get \"{m.FileName}\" -c \"{m.Channel}\" -o \"{d.FileName}\"");
            Footer.Text = reply.Split('\n')[0].Trim();
            Footer.Foreground = reply.StartsWith("collab:") ? Sol.Red : Sol.Green;
        }

        void OnUpdate(object s, RoutedEventArgs e) => Updater.CheckForUpdates(this);

        void OnChannels(object s, RoutedEventArgs e)
        {
            new ChannelsWindow { Owner = this }.ShowDialog();
            core.LoadWho();
        }
    }
}
