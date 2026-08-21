// Puts colour emoji into an ordinary TextBlock.
//
// Rather than write a text control — which would mean reimplementing word
// wrapping, selection and hit testing — the string is split into runs and each
// emoji becomes an inline image. WPF keeps doing the layout it is good at.
using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;

namespace Collab
{
    public static class EmojiText
    {
        public static readonly DependencyProperty SourceProperty =
            DependencyProperty.RegisterAttached(
                "Source", typeof(string), typeof(EmojiText),
                new PropertyMetadata(null, OnChanged));

        public static void SetSource(DependencyObject o, string v) => o.SetValue(SourceProperty, v);
        public static string GetSource(DependencyObject o) => (string)o.GetValue(SourceProperty);

        static void OnChanged(DependencyObject o, DependencyPropertyChangedEventArgs e)
        {
            if (o is not TextBlock tb) return;
            Fill(tb, e.NewValue as string ?? "");
        }

        static void Fill(TextBlock tb, string text)
        {
            tb.Inlines.Clear();
            if (text.Length == 0) return;
            if (!EmojiFont.Available) { tb.Inlines.Add(new Run(text)); return; }

            double size = tb.FontSize > 0 ? tb.FontSize : 13;
            var buffer = new System.Text.StringBuilder();

            void FlushText()
            {
                if (buffer.Length == 0) return;
                tb.Inlines.Add(new Run(buffer.ToString()));
                buffer.Clear();
            }

            for (int i = 0; i < text.Length;)
            {
                int cp = char.ConvertToUtf32(text, i);
                int width = char.IsSurrogatePair(text, i) ? 2 : 1;

                // Joiners and variation selectors have no glyph of their own and
                // would draw as boxes. Dropping them means a joined sequence
                // renders as its parts rather than as one picture — a known
                // limit, and better than a row of squares.
                if (cp == 0x200D || cp == 0xFE0F || cp == 0xFE0E) { i += width; continue; }

                var img = EmojiFont.Render(cp, size);
                if (img == null) { buffer.Append(text, i, width); i += width; continue; }

                FlushText();
                tb.Inlines.Add(new InlineUIContainer(new Image
                {
                    Source = img,
                    Width = size * 1.25,
                    Height = size * 1.25,
                    Stretch = Stretch.Uniform,
                    Margin = new Thickness(0, 0, 0, -size * 0.2),
                })
                { BaselineAlignment = BaselineAlignment.Baseline });
                i += width;
            }
            FlushText();
        }
    }
}
