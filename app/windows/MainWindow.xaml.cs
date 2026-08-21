using System;
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

        public MainWindow()
        {
            InitializeComponent();
            Loaded += OnLoaded;
            Closed += (_, _) => core.Stop();
            Activated += (_, _) =>
            {
                // Windows has no notification worth the plumbing for this, so
                // the theme is re-read whenever the window comes forward.
                var dark = Sol.SystemPrefersDark();
                if (dark != Sol.Dark) { Sol.Dark = dark; Paint(); Rebuild(); }
            };
        }

        void OnLoaded(object? s, RoutedEventArgs e)
        {
            Paint();
            core.Changed += () => Dispatcher.Invoke(() => { Sync(); });
            core.Arrived += OnArrived;
            core.Start();
            Entry.Focus();
        }

        /// Everything the theme touches, in one place.
        void Paint()
        {
            Background = Sol.Bg;
            Root.Background = Sol.Bg;
            ServerLabel.Foreground = Sol.FgDim;
            Entry.Background = Sol.BgAlt;
            Entry.Foreground = Sol.FgEm;
            Entry.BorderBrush = Sol.Rule;
            Entry.CaretBrush = Sol.FgEm;
            SendBtn.Background = Sol.Blue;
            SendBtn.Foreground = Sol.OnAccent;
            SendBtn.BorderBrush = Sol.Blue;
            ChannelPicker.Background = Sol.BgAlt;
            ChannelPicker.Foreground = Sol.FgEm;
            PaintTabs();
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
            Dot.Fill = core.Connected ? Sol.Green : Sol.Red;
            ServerLabel.Text = core.Fatal ?? (core.Connected
                ? core.ServerAddr
                : "DISCONNECTED from " + core.ServerAddr + " — retrying");
            if (ChannelPicker.Items.Count != core.Channels.Count)
            {
                var keep = channel;
                ChannelPicker.Items.Clear();
                foreach (var c in core.Channels) ChannelPicker.Items.Add(c);
                var pick = core.Channels.Contains(keep) ? keep : core.Channels.FirstOrDefault() ?? "";
                channel = pick;
                ChannelPicker.SelectedItem = pick;
            }
            Rebuild();
        }

        void Rebuild()
        {
            if (string.IsNullOrEmpty(channel)) return;
            var rows = core.Messages
                .Where(m => m.Channel == channel)
                .Where(m => view == "changes" ? m.IsChange : !m.IsChange)
                .ToList();
            List.ItemsSource = rows;
            Entry.IsEnabled = view == "chat";
            SendBtn.IsEnabled = view == "chat";
            Scroller.ScrollToEnd();
        }

        void OnArrived(Msg m)
        {
            // Not your own words read back at you, and not a view you are
            // already looking at with the window in front of you.
            if (m.Who == core.Me && !m.IsAI) return;
            if (IsActive && m.Channel == channel) return;
            Toast.Post("collab", "#" + m.Channel + " · " + m.Who, m.Line);
        }

        void OnChat(object s, RoutedEventArgs e) { view = "chat"; PaintTabs(); Rebuild(); }
        void OnChanges(object s, RoutedEventArgs e) { view = "changes"; PaintTabs(); Rebuild(); }

        void OnChannel(object s, SelectionChangedEventArgs e)
        {
            if (ChannelPicker.SelectedItem is string c && c != channel) { channel = c; Rebuild(); }
        }

        void OnKey(object s, KeyEventArgs e)
        {
            if (e.Key == Key.Enter && Keyboard.Modifiers == ModifierKeys.None) { OnSend(s, e); e.Handled = true; }
        }

        void OnSend(object s, RoutedEventArgs e)
        {
            var text = Entry.Text.Trim();
            if (text.Length == 0 || string.IsNullOrEmpty(channel)) return;
            Entry.Text = "";
            // The composer posts to the channel on screen, not the one it
            // started on — the Mac app got that wrong once and it was
            // thoroughly confusing.
            var target = channel;
            var reply = core.Post(target, text);
            if (reply.StartsWith("collab:") || reply.Contains("REFUSED"))
                ServerLabel.Text = reply.Split('\n')[0];
        }
    }
}
