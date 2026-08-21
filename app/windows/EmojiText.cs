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
            var text = e.NewValue as string ?? "";
            switch (o)
            {
                case TextBlock tb:
                    tb.Inlines.Clear();
                    foreach (var i in Build(text, tb.FontSize)) tb.Inlines.Add(i);
                    break;
                // A RichTextBox so the text can be selected and copied.
                // TextBlock cannot do it at all — WPF has no equivalent of
                // WinUI's IsTextSelectionEnabled — and a plain TextBox cannot
                // hold the inline images the emoji are made of.
                case RichTextBox rtb:
                    var para = new Paragraph { Margin = new Thickness(0) };
                    foreach (var i in Build(text, rtb.FontSize)) para.Inlines.Add(i);
                    rtb.Document = new FlowDocument(para)
                    {
                        PagePadding = new Thickness(0),
                        FontFamily = rtb.FontFamily,
                        FontSize = rtb.FontSize,
                    };
                    break;
            }
        }

        static IEnumerable<Inline> Build(string text, double fontSize)
        {
            var made = new List<Inline>();
            if (text.Length == 0) return made;
            if (!EmojiFont.Available) { made.Add(new Run(text)); return made; }

            double size = fontSize > 0 ? fontSize : 13;
            var buffer = new System.Text.StringBuilder();

            void FlushText()
            {
                if (buffer.Length == 0) return;
                made.Add(new Run(buffer.ToString()));
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
                made.Add(new InlineUIContainer(new Image
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
            return made;
        }
    }
}
