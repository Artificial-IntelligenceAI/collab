// Puts colour emoji into an ordinary TextBlock or RichTextBox.
//
// Two jobs, and both are handed to something that already does them properly.
// Where one emoji ends and the next begins is a Unicode grapheme-cluster
// question, and .NET answers it — including 👨‍👩‍👧 as one cluster, 🇹🇭 as one, and
// 👍🏽 as one. Drawing the cluster is DirectWrite's job, through EmojiDW.
//
// The previous version did both by hand: it mapped single codepoints through
// the font's cmap and applied the ligature table itself. That reached simple
// emoji and skin tones and could not reach anything joined, because the font
// expects contextual substitutions to run first.
//
// Rather than a custom text control — which would mean reimplementing word
// wrapping, selection and hit testing — the string becomes runs and inline
// images, and WPF keeps doing the layout it is good at.
using System;
using System.Collections.Generic;
using System.Globalization;
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

        static readonly Dictionary<(string, int), ImageSource?> cache = new();

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

        /// Whether a grapheme cluster is something to draw as a picture. Broad
        /// on purpose: a cluster that is not really an emoji renders as itself
        /// through DirectWrite anyway, so a false positive costs a little
        /// memory, while a false negative shows a box.
        static bool LooksLikeEmoji(string cluster)
        {
            for (int i = 0; i < cluster.Length;)
            {
                int cp = char.ConvertToUtf32(cluster, i);
                i += char.IsSurrogatePair(cluster, i) ? 2 : 1;
                if (cp >= 0x1F000) return true;                    // the emoji planes
                if (cp is >= 0x2600 and <= 0x27BF) return true;     // misc symbols, dingbats
                if (cp is >= 0x2B00 and <= 0x2BFF) return true;     // arrows and shapes
                if (cp is >= 0x2190 and <= 0x21FF) return true;     // arrows
                if (cp == 0xFE0F || cp == 0x20E3) return true;      // emoji presentation, keycap
                if (cp is 0x00A9 or 0x00AE or 0x2122) return true;  // © ® ™
            }
            return false;
        }

        static IEnumerable<Inline> Build(string text, double fontSize)
        {
            var made = new List<Inline>();
            if (text.Length == 0) return made;
            double size = fontSize > 0 ? fontSize : 13;
            if (!EmojiDW.Available) { made.Add(new Run(text)); return made; }

            var buffer = new System.Text.StringBuilder();
            void FlushText()
            {
                if (buffer.Length == 0) return;
                made.Add(new Run(buffer.ToString()));
                buffer.Clear();
            }

            var e = StringInfo.GetTextElementEnumerator(text);
            while (e.MoveNext())
            {
                var cluster = (string)e.Current;
                if (!LooksLikeEmoji(cluster)) { buffer.Append(cluster); continue; }

                var key = (cluster, (int)Math.Round(size));
                if (!cache.TryGetValue(key, out var img))
                {
                    img = EmojiDW.Render(cluster, size);
                    cache[key] = img;
                }
                if (img == null) { buffer.Append(cluster); continue; }

                FlushText();
                made.Add(new InlineUIContainer(new Image
                {
                    Source = img,
                    Width = size * 1.35,
                    Height = size * 1.35,
                    Stretch = Stretch.Uniform,
                    Margin = new Thickness(0, 0, 0, -size * 0.25),
                })
                { BaselineAlignment = BaselineAlignment.Baseline });
            }
            FlushText();
            return made;
        }
    }
}
