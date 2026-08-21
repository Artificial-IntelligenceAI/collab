// One question, asked when a channel first appears: what should people call you
// here. A machine name is nobody's choice — it is whatever the computer was
// called when it was set up — and the same person is reasonably a different
// name to their family and to a work project.
using System.Windows;
using System.Windows.Controls;

namespace Collab
{
    public static class NamePrompt
    {
        public static string? Ask(Window owner, string channel, string suggested)
        {
            var box = new TextBox
            {
                Text = suggested, Padding = new Thickness(7, 5, 7, 5), FontSize = 13,
                Background = Sol.BgAlt, Foreground = Sol.FgEm,
                BorderBrush = Sol.Rule, CaretBrush = Sol.FgEm,
            };
            var note = new TextBlock
            {
                Text = "This is the name the other machine sees, and the one they can @ you by. "
                     + "It applies to this channel only.",
                TextWrapping = TextWrapping.Wrap, FontSize = 11,
                Foreground = Sol.FgDim, Margin = new Thickness(0, 8, 0, 0),
            };
            var ok = new Button { Content = "Use this name", Padding = new Thickness(16, 5, 16, 5), IsDefault = true,
                                  Background = Sol.Blue, Foreground = Sol.OnAccent, BorderBrush = Sol.Blue };
            var skip = new Button { Content = "Skip", Padding = new Thickness(14, 5, 14, 5),
                                    Margin = new Thickness(8, 0, 0, 0), IsCancel = true };

            var buttons = new StackPanel { Orientation = Orientation.Horizontal,
                                           HorizontalAlignment = HorizontalAlignment.Right,
                                           Margin = new Thickness(0, 14, 0, 0) };
            buttons.Children.Add(ok); buttons.Children.Add(skip);

            var panel = new StackPanel { Margin = new Thickness(18) };
            panel.Children.Add(new TextBlock
            {
                Text = "What should people call you on #" + channel + "?",
                FontWeight = FontWeights.SemiBold, FontSize = 14,
                Foreground = Sol.FgEm, Margin = new Thickness(0, 0, 0, 10),
            });
            panel.Children.Add(box); panel.Children.Add(note); panel.Children.Add(buttons);

            var w = new Window
            {
                Title = "collab", Content = panel, Owner = owner, Background = Sol.Bg,
                SizeToContent = SizeToContent.Height, Width = 420, ResizeMode = ResizeMode.NoResize,
                WindowStartupLocation = WindowStartupLocation.CenterOwner, ShowInTaskbar = false,
            };
            string? answer = null;
            ok.Click += (_, _) => { answer = box.Text.Trim(); w.Close(); };
            skip.Click += (_, _) => { answer = null; w.Close(); };
            w.Loaded += (_, _) => { box.Focus(); box.SelectAll(); };
            w.ShowDialog();
            return string.IsNullOrWhiteSpace(answer) ? null : answer;
        }
    }
}
