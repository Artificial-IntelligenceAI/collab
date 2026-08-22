// Puts colour emoji into an ordinary TextBlock, including the joined ones.
//
// Rather than write a text control — which would mean reimplementing word
// wrapping, selection and hit testing — the string is split into runs and each
// emoji becomes an inline image. WPF keeps doing the layout it is good at.
//
// A joined emoji is several codepoints that the font turns into one glyph:
// 👨‍👩‍👧 is man, zero-width joiner, woman, joiner, girl, and 👍🏽 is a thumb plus a
// skin tone. The font's GSUB table says which sequences collapse; without
// applying it the parts each render separately, which is what this used to do.
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

        /// One codepoint of the source, with where it came from, so a run that
        /// turns out not to be an emoji can be put back as the original text.
        readonly struct Unit
        {
            public readonly int Cp, Index, Length;
            public readonly ushort? Glyph;
            public Unit(int cp, int index, int length, ushort? glyph)
            { Cp = cp; Index = index; Length = length; Glyph = glyph; }
        }

        static IEnumerable<Inline> Build(string text, double fontSize)
        {
            var made = new List<Inline>();
            if (text.Length == 0) return made;
            if (!EmojiFont.Available) { made.Add(new Run(text)); return made; }

            double size = fontSize > 0 ? fontSize : 13;

            var units = new List<Unit>();
            for (int i = 0; i < text.Length;)
            {
                int cp = char.ConvertToUtf32(text, i);
                int len = char.IsSurrogatePair(text, i) ? 2 : 1;
                units.Add(new Unit(cp, i, len, EmojiFont.Glyph(cp)));
                i += len;
            }

            var buffer = new System.Text.StringBuilder();
            void FlushText()
            {
                if (buffer.Length == 0) return;
                made.Add(new Run(buffer.ToString()));
                buffer.Clear();
            }

            for (int i = 0; i < units.Count;)
            {
                var u = units[i];

                // Try the longest ligature starting here first, so a family is
                // one picture rather than three people.
                if (u.Glyph is ushort g && EmojiFont.StartsLigature(g))
                {
                    var run = new List<ushort>();
                    for (int k = i; k < units.Count && units[k].Glyph is ushort gk; k++) run.Add(gk);
                    var hit = EmojiFont.Ligate(run, 0);
                    if (hit is (ushort lig, int used) && EmojiFont.HasColour(lig))
                    {
                        FlushText();
                        made.Add(Picture(lig, size));
                        i += used;
                        continue;
                    }
                }

                // A joiner or variation selector that did not form a ligature
                // has no glyph worth drawing and would show as a box.
                if (u.Cp == 0x200D || u.Cp == 0xFE0F || u.Cp == 0xFE0E) { i++; continue; }

                if (u.Glyph is ushort single && EmojiFont.HasColour(single))
                {
                    FlushText();
                    made.Add(Picture(single, size));
                    i++;
                    continue;
                }

                buffer.Append(text, u.Index, u.Length);
                i++;
            }
            FlushText();
            return made;
        }

        static Inline Picture(ushort glyph, double size)
        {
            var img = EmojiFont.RenderGlyph(glyph, size);
            if (img == null) return new Run("");
            return new InlineUIContainer(new Image
            {
                Source = img,
                Width = size * 1.25,
                Height = size * 1.25,
                Stretch = Stretch.Uniform,
                Margin = new Thickness(0, 0, 0, -size * 0.2),
            })
            { BaselineAlignment = BaselineAlignment.Baseline };
        }
    }
}
