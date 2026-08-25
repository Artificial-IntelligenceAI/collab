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

        /// One stretch of text with the inline markdown that applies to it.
        readonly struct Seg
        {
            public readonly string Text; public readonly bool Bold, Italic, Code;
            public Seg(string t, bool b, bool i, bool c) { Text = t; Bold = b; Italic = i; Code = c; }
        }

        /// Inline markdown only: `code`, **bold**, *italic*. No headers, lists or
        /// block quotes — a chat line is not a document, and a sentence opening
        /// with "# " should keep its hash.
        ///
        /// A delimiter with no partner stays literal, so "2 * 3" and a lone
        /// backtick read as themselves rather than swallowing the rest of the
        /// message. Code wins outright: nothing inside backticks is markup,
        /// which matters here because a backticked name is how you write about
        /// somebody without addressing them.
        static List<Seg> Split(string text)
        {
            var segs = new List<Seg>();
            var buf = new System.Text.StringBuilder();
            bool bold = false, italic = false;
            void Flush() { if (buf.Length > 0) { segs.Add(new Seg(buf.ToString(), bold, italic, false)); buf.Clear(); } }

            int i = 0;
            while (i < text.Length)
            {
                char c = text[i];
                if (c == '`')
                {
                    int end = text.IndexOf('`', i + 1);
                    if (end > i)
                    {
                        Flush();
                        segs.Add(new Seg(text.Substring(i + 1, end - i - 1), bold, italic, true));
                        i = end + 1; continue;
                    }
                }
                else if (c == '*' && i + 1 < text.Length && text[i + 1] == '*')
                {
                    // Closing always closes; opening only opens if a closer exists.
                    if (bold) { Flush(); bold = false; i += 2; continue; }
                    if (text.IndexOf("**", i + 2, StringComparison.Ordinal) > 0) { Flush(); bold = true; i += 2; continue; }
                }
                else if (c == '*')
                {
                    if (italic) { Flush(); italic = false; i += 1; continue; }
                    if (text.IndexOf('*', i + 1) > 0) { Flush(); italic = true; i += 1; continue; }
                }
                buf.Append(c); i++;
            }
            Flush();
            return segs;
        }

        static readonly FontFamily Mono = new FontFamily("Cascadia Mono, Consolas, Courier New");

        /// Who this window is, so the mention aimed at it can be picked out of the
        /// ones aimed at everybody else. Set once by the main window; empty just
        /// means every mention is drawn the same, which is what happened before.
        public static string Me = "";

        /// A name written after an @, trimmed of the punctuation that ends a
        /// sentence rather than a name. Mirrors the Mac's reading of the same
        /// text so both windows agree on where a mention stops.
        static string MentionName(string word)
        {
            var n = word.TrimStart('@');
            int end = n.Length;
            while (end > 0 && !(char.IsLetterOrDigit(n[end - 1]) || "-_./".IndexOf(n[end - 1]) >= 0)) end--;
            return n.Substring(0, end).ToLowerInvariant();
        }

        static IEnumerable<Inline> Build(string text, double fontSize)
        {
            var made = new List<Inline>();
            if (text.Length == 0) return made;
            double size = fontSize > 0 ? fontSize : 13;
            if (!EmojiDW.Available) { made.Add(new Run(text)); return made; }

            // Fenced blocks first, then inline markup inside the prose between
            // them. Until today no message could hold a newline at all — they
            // were replaced with spaces before storage — so every diagram and
            // table posted here arrived as one flowed line. Now that the text
            // survives, a block has to keep its shape: alignment is the whole
            // content of a diagram, and losing it leaves every character
            // present and the meaning gone.
            bool first = true;
            foreach (var block in Fences(text))
            {
                if (!first) made.Add(new LineBreak());
                first = false;
                if (block.code)
                {
                    // Shaded runs in the flow, NOT a bordered box.
                    //
                    // The box was the Mac's look and it cost the thing this
                    // window was fixed for: an InlineUIContainer sits outside
                    // the RichTextBox's selection model, so the block could not
                    // be selected or copied, and the TextBlock inside drew a
                    // second caret of its own. Tankun saw two cursors and could
                    // not copy the diagram.
                    //
                    // Shading each run keeps the block visually distinct while
                    // leaving it ordinary text: selectable, copyable, one
                    // caret. Parity with the Mac is worth less than being able
                    // to copy what you are reading.
                    var lines = block.text.Split('\n');
                    for (int i = 0; i < lines.Length; i++)
                    {
                        if (i > 0) made.Add(new LineBreak());
                        made.Add(new Run(lines[i])
                        {
                            FontFamily = Mono,
                            Foreground = Sol.FgEm,
                            Background = Sol.BgAlt,
                            FontSize = size - 1,
                        });
                    }
                }
                else
                {
                    var lines = block.text.Split('\n');
                    for (int i = 0; i < lines.Length; i++)
                    {
                        if (i > 0) made.Add(new LineBreak());
                        foreach (var seg in Split(lines[i])) BuildSeg(seg, size, made);
                    }
                }
            }
            return made;
        }

        /// Splits on ``` fences. An unclosed fence stays prose rather than
        /// swallowing the rest of the message — a half-typed block should look
        /// wrong, not make everything after it vanish into a box.
        static List<(string text, bool code)> Fences(string text)
        {
            var outp = new List<(string, bool)>();
            var lines = text.Split('\n');
            bool any = false;
            foreach (var l in lines) if (l.TrimStart().StartsWith("```")) { any = true; break; }
            if (!any) { outp.Add((text, false)); return outp; }

            var buf = new List<string>();
            bool inCode = false;
            void Flush(bool code)
            {
                var joined = string.Join("\n", buf);
                if (joined.Trim().Length > 0) outp.Add((joined, code));
                buf.Clear();
            }
            foreach (var l in lines)
            {
                if (l.TrimStart().StartsWith("```")) { Flush(inCode); inCode = !inCode; continue; }
                buf.Add(l);
            }
            Flush(false);
            return outp;
        }

        static void BuildSeg(Seg seg, double size, List<Inline> made)
        {
            string text = seg.Text;
            Run Style(Run r)
            {
                if (seg.Bold) r.FontWeight = FontWeights.Bold;
                if (seg.Italic) r.FontStyle = FontStyles.Italic;
                if (seg.Code) { r.FontFamily = Mono; r.Foreground = Sol.Cyan; r.FontSize = size - 1; }
                return r;
            }

            var buffer = new System.Text.StringBuilder();
            void FlushText()
            {
                if (buffer.Length == 0) return;
                made.Add(Style(new Run(buffer.ToString())));
                buffer.Clear();
            }

            // Mentions are coloured only outside code. A live @name reaches
            // somebody and a backticked one addresses nobody, so drawing them
            // alike would hide the only difference that matters.
            if (!seg.Code && text.IndexOf('@') >= 0)
            {
                int i = 0;
                while (i < text.Length)
                {
                    bool atWordStart = i == 0 || char.IsWhiteSpace(text[i - 1]);
                    if (text[i] != '@' || !atWordStart) { buffer.Append(text[i]); i++; continue; }
                    int end = i + 1;
                    while (end < text.Length && !char.IsWhiteSpace(text[end])) end++;
                    var word = text.Substring(i, end - i);
                    var name = MentionName(word);
                    if (name.Length == 0) { buffer.Append(text[i]); i++; continue; }
                    FlushText();
                    var run = Style(new Run(word));
                    run.FontWeight = FontWeights.SemiBold;
                    bool mine = Me.Length > 0 && name == Me.ToLowerInvariant();
                    run.Foreground = mine ? Sol.OnAccent : Sol.Blue;
                    if (mine) run.Background = Sol.Blue;
                    made.Add(run);
                    i = end;
                }
                FlushText();
                return;
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
        }
    }
}
