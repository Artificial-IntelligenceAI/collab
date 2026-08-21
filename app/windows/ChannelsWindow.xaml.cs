// Making and sharing channels — the one thing here that only a person does.
// An AI cannot make one, because a key that has not been carried to the other
// machine is a room with nobody in it.
using System;
using System.Linq;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Collab
{
    public partial class ChannelsWindow : Window
    {
        public ChannelsWindow()
        {
            InitializeComponent();
            Loaded += (_, _) => { Paint(); Reload(); };
        }

        void Paint()
        {
            Background = Sol.Bg;
            foreach (var t in new TextBlock[] { Blurb, MakeLabel, JoinLabel })
                t.Foreground = t == Blurb ? Sol.FgDim : Sol.FgEm;
            JoinKeyHint.Foreground = Sol.FgDim;
            foreach (var b in new TextBox[] { NewName, JoinKey })
            {
                b.Background = Sol.BgAlt; b.Foreground = Sol.FgEm;
                b.BorderBrush = Sol.Rule; b.CaretBrush = Sol.FgEm;
            }
            MakeBtn.Background = Sol.Blue; MakeBtn.Foreground = Sol.OnAccent; MakeBtn.BorderBrush = Sol.Blue;
            Note.Foreground = Sol.FgDim;
        }

        void Reload()
        {
            Rows.Children.Clear();
            // -keys explicitly: this panel exists to show a key to a person so
            // they can send it to the other machine. Everywhere else asks
            // without it.
            var json = Core.Run("channels -json -keys");
            try
            {
                var arr = JsonDocument.Parse(json).RootElement;
                if (arr.GetArrayLength() == 0)
                {
                    Rows.Children.Add(new TextBlock
                    {
                        Text = "No channels yet. Make one below, then send its key to the other person.",
                        Foreground = Sol.FgDim, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0),
                    });
                    return;
                }
                foreach (var c in arr.EnumerateArray()) Rows.Children.Add(Row(c));
            }
            catch (Exception e)
            {
                Rows.Children.Add(new TextBlock { Text = "cannot read the channel list — " + e.Message,
                                                  Foreground = Sol.Red, TextWrapping = TextWrapping.Wrap });
            }
        }

        UIElement Row(JsonElement c)
        {
            string name = c.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            string key = c.TryGetProperty("key", out var k) ? k.GetString() ?? "" : "";
            string invite = c.TryGetProperty("invite", out var iv) ? iv.GetString() ?? key : key;
            bool mine = c.TryGetProperty("mine", out var m) && m.GetBoolean();

            var head = new StackPanel { Orientation = Orientation.Horizontal };
            head.Children.Add(new TextBlock { Text = "#" + name, FontWeight = FontWeights.SemiBold,
                                              Foreground = Sol.ForName(name), FontSize = 13 });
            head.Children.Add(new TextBlock { Text = mine ? "made here" : "joined", FontSize = 11,
                                              Foreground = Sol.FgDim, Margin = new Thickness(8, 2, 0, 0) });

            var keyLine = new TextBlock { Text = invite, FontFamily = new FontFamily("Consolas"), FontSize = 11,
                                          Foreground = Sol.FgDim, TextWrapping = TextWrapping.Wrap,
                                          Margin = new Thickness(0, 3, 0, 0) };

            var youAre = new TextBlock
            {
                Text = "you are “" + (Core.DisplayOn(name) ?? Core.MachineName) + "” here",
                FontSize = 11, Foreground = Sol.FgDim, Margin = new Thickness(0, 4, 0, 0),
            };

            var rename = new Button { Content = "Change name", Padding = new Thickness(10, 3, 10, 3), Margin = new Thickness(0, 0, 6, 0) };
            rename.Click += (_, _) =>
            {
                var chosen = NamePrompt.Ask(this, name, Core.DisplayOn(name) ?? Core.MachineName);
                if (chosen == null) return;
                Core.Run($"channel name \"{name}\" \"{chosen}\"");
                Reload();
            };

            var copy = new Button { Content = "Copy invite", Padding = new Thickness(10, 3, 10, 3), Margin = new Thickness(0, 0, 6, 0) };
            copy.Click += (_, _) => { try { Clipboard.SetText(invite); copy.Content = "Copied"; } catch { } };

            var drop = new Button { Content = mine ? "Delete" : "Forget", Padding = new Thickness(10, 3, 10, 3) };
            drop.Click += (_, _) =>
            {
                var q = mine
                    ? $"Delete #{name} everywhere?\n\nIts messages and files go with it, on both machines. This cannot be undone."
                    : $"Leave #{name}?\n\nThis drops your key only. The channel carries on without you.";
                if (MessageBox.Show(q, "collab", MessageBoxButton.OKCancel, MessageBoxImage.Warning) != MessageBoxResult.OK) return;
                Note.Text = Core.Run($"channel {(mine ? "delete" : "forget")} \"{name}\"").Split('\n')[0].Trim();
                Reload();
            };

            var buttons = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 6, 0, 0) };
            buttons.Children.Add(rename); buttons.Children.Add(copy); buttons.Children.Add(drop);

            var body = new StackPanel();
            body.Children.Add(head); body.Children.Add(youAre);
            body.Children.Add(keyLine); body.Children.Add(buttons);

            return new Border
            {
                Background = Sol.BgAlt, BorderBrush = Sol.Rule, BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(6), Padding = new Thickness(11, 9, 11, 9),
                Margin = new Thickness(0, 0, 0, 8), Child = body,
            };
        }

        void OnMake(object s, RoutedEventArgs e)
        {
            var name = NewName.Text.Trim();
            if (name.Length == 0) return;
            var outp = Core.Run($"channel create \"{name}\"").Trim();
            NewName.Text = "";
            if (!outp.StartsWith("collab:")) AskName(name);
            Note.Text = outp.Split('\n')[0];
            Note.Foreground = outp.StartsWith("collab:") ? Sol.Red : Sol.Green;
            Reload();
        }

        /// One argument. The invite carries the name, so joining is joining —
        /// nobody is asked what to call a room somebody else made.
        void OnJoin(object s, RoutedEventArgs e)
        {
            var invite = JoinKey.Text.Trim();
            if (invite.Length == 0) return;
            var outp = Core.Run($"channel add \"{invite}\"").Trim();
            Note.Text = outp.Split('\n')[0];
            Note.Foreground = outp.StartsWith("collab:") ? Sol.Red : Sol.Green;
            if (!outp.StartsWith("collab:"))
            {
                JoinKey.Text = "";
                // The name of the channel just joined is the first word of what
                // the CLI printed: `joined #roblox-game — …`
                var joined = outp.Split(' ').FirstOrDefault(w => w.StartsWith("#"))?.TrimStart('#');
                if (!string.IsNullOrEmpty(joined)) AskName(joined);
            }
            Reload();
        }

        /// Asked when a channel first appears, because that is the moment a
        /// person knows who they are talking to. Skipping it leaves the machine
        /// name, which is whatever the computer was called in a shop.
        void AskName(string channel)
        {
            var chosen = NamePrompt.Ask(this, channel, Core.MachineName);
            if (!string.IsNullOrWhiteSpace(chosen))
                Core.Run($"channel name \"{channel}\" \"{chosen}\"");
        }

        void OnDone(object s, RoutedEventArgs e) => Close();
    }
}
