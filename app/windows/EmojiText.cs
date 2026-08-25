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
using System.IO;
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
                    docFamily = rtb.FontFamily;
                    var doc = new FlowDocument
                    {
                        PagePadding = new Thickness(0),
                        FontFamily = rtb.FontFamily,
                        FontSize = rtb.FontSize,
                        // Ragged right, not justified.
                        //
                        // FlowDocument justifies by default, and justification
                        // stretches the spaces in a line — which would shear a
                        // table by exactly the rows that contain text. Kept
                        // because chat is read ragged-right everywhere else and
                        // because a monospace block must never be stretched.
                        //
                        // Honesty about why this was written: it was added while
                        // chasing a misaligned block that turned out to be a
                        // typo in the test message — its border rows were 31
                        // characters and its text row 32. Justification was not
                        // the cause of anything observed. It is right anyway.
                        TextAlignment = TextAlignment.Left,
                    };
                    foreach (var b in BuildBlocks(text, rtb.FontSize)) doc.Blocks.Add(b);
                    rtb.Document = doc;
                    break;
            }
        }

        /// Whether a grapheme cluster is something to draw as a picture. Broad
        /// on purpose: a cluster that is not really an emoji renders as itself
        /// through DirectWrite anyway, so a false positive costs a little
        /// memory, while a false negative shows a box.
        static bool LooksLikeEmoji(string cluster) => LooksLikeEmoji(cluster, false);

        /// `inBlock` narrows it to things that are emoji by default.
        ///
        /// Being generous is right in prose — a cluster that is not really an
        /// emoji renders as itself anyway. In a monospace block it is not: an
        /// arrow drawn as a picture takes two cells where the font gives it one,
        /// and the row shears. U+2190..21FF is the arrows block, U+2600..27BF
        /// holds dingbats and U+2B00..2BFF holds geometric shapes; those are
        /// text by default and JetBrains Mono draws every one of them at exactly
        /// one cell.
        ///
        /// So inside a block only the emoji planes count, plus anything wearing
        /// an explicit emoji-presentation selector — which is a person asking
        /// for the picture.
        static bool LooksLikeEmoji(string cluster, bool inBlock)
        {
            if (inBlock)
            {
                bool pictorial = false;
                for (int i = 0; i < cluster.Length;)
                {
                    int cp = char.ConvertToUtf32(cluster, i);
                    i += char.IsSurrogatePair(cluster, i) ? 2 : 1;
                    if (cp >= 0x1F000 || cp == 0xFE0F) pictorial = true;
                }
                return pictorial;
            }
            return LooksLikeEmojiLoose(cluster);
        }

        static bool LooksLikeEmojiLoose(string cluster)
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

        /// A message as document blocks, one paragraph per fenced block.
        ///
        /// A paragraph's Background paints the whole block, edge to edge and
        /// across every line. Shading the runs instead followed each line's own
        /// width, so a diagram came out as a staircase of different-length
        /// stripes — which is what Tankun saw, and worse than no shading.
        ///
        /// And a paragraph is still text. The bordered box before it was an
        /// InlineUIContainer, which sits outside the selection model: it could
        /// not be copied and drew a second caret. This keeps the block visually
        /// distinct while leaving every character selectable.
        static IEnumerable<Block> BuildBlocks(string text, double size)
        {
            ReportFont(size);
            foreach (var block in Fences(text))
            {
                var lines = block.text.Split('\n');
                if (block.code)
                {
                    var p = new Paragraph
                    {
                        TextAlignment = TextAlignment.Left,
                        Margin = new Thickness(0, 3, 0, 3),
                        Padding = new Thickness(9, 6, 9, 6),
                        Background = Sol.BgAlt,
                        // The edge the Mac draws. A shaded rectangle with no
                        // rule reads as tinted prose; the border is what says
                        // "this is a block" before anything is read. WPF's Block
                        // has no corner radius, so the Mac's is rounded and this
                        // is not — as close as the two toolkits allow.
                        BorderBrush = Sol.Rule,
                        BorderThickness = new Thickness(1),
                        FontFamily = Mono,
                        FontSize = size - 1,
                        Foreground = Sol.FgEm,
                    };
                    for (int i = 0; i < lines.Length; i++)
                    {
                        if (i > 0) p.Inlines.Add(new LineBreak());
                        // Through the emoji path, not as a plain Run. A block is
                        // still text somebody wrote, and an emoji in a diagram
                        // was coming out as a monochrome glyph while the same
                        // character three words earlier drew in colour.
                        //
                        // Nothing inside a block is markup, so the segment is
                        // marked code and its runs are then recoloured to the
                        // block's own foreground rather than inline code's cyan.
                        var made = new List<Inline>();
                        BuildSeg(new Seg(lines[i], false, false, true), size, made);
                        foreach (var inl in made)
                        {
                            if (inl is Run r) r.Foreground = Sol.FgEm;
                            p.Inlines.Add(inl);
                        }
                    }
                    yield return p;
                }
                else
                {
                    // Prose, with any markdown tables lifted out of it. A table
                    // is a row of cells followed by a |---|---| line; both are
                    // required, because a lone line containing pipes is a
                    // sentence about pipes.
                    var p = new Paragraph { Margin = new Thickness(0), TextAlignment = TextAlignment.Left };
                    bool started = false;
                    for (int i = 0; i < lines.Length; i++)
                    {
                        List<TextAlignment>? align = null;
                        if (lines[i].Contains("|") && i + 1 < lines.Length)
                        {
                            align = SeparatorAlignments(lines[i + 1]);
                            if (align != null && SplitRow(lines[i]).Count != align.Count) align = null;
                        }
                        if (align != null)
                        {
                            if (started) yield return p;
                            p = new Paragraph { Margin = new Thickness(0), TextAlignment = TextAlignment.Left };
                            started = false;

                            var rows = new List<List<string>> { SplitRow(lines[i]) };
                            int j = i + 2;
                            while (j < lines.Length && lines[j].Contains("|")
                                   && !lines[j].TrimStart().StartsWith("```"))
                            {
                                rows.Add(SplitRow(lines[j]));
                                j++;
                            }
                            yield return BuildTable(rows, align, size);
                            i = j - 1;
                            continue;
                        }
                        if (started) p.Inlines.Add(new LineBreak());
                        started = true;
                        var made = new List<Inline>();
                        foreach (var seg in Split(lines[i])) BuildSeg(seg, size, made);
                        foreach (var inl in made) p.Inlines.Add(inl);
                    }
                    if (started) yield return p;
                }
            }
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

        /// Bundled, not looked up. The old stack named Cascadia Mono first and
        /// this machine does not have it, so every block silently rendered in
        /// Consolas while the Mac used SF Mono — the same diagram, two shapes,
        /// and nothing to say so. Carrying the file is the only way the two
        /// windows agree about what a monospace column is.
        /// The font the document is actually drawn in, so a cell is measured
        /// against the face that will render it rather than against a guess.
        static FontFamily? docFamily;

        static readonly FontFamily Mono = new FontFamily(
            new Uri("pack://application:,,,/"), "./Fonts/#JetBrains Mono");

        /// What the block font actually resolved to, and what one cell measures.
        ///
        /// A pack URI that does not resolve does not throw — WPF falls back, and
        /// the fallback is proportional, which shears a table while leaving every
        /// character present. Written out once so the question can be answered
        /// from outside the window rather than from a screenshot of it.
        static bool reported;
        internal static void ReportFont(double size)
        {
            if (reported) return;
            reported = true;
            try
            {
                var lines = new System.Collections.Generic.List<string>();
                var names = string.Join(", ", Mono.FamilyNames.Values);
                lines.Add("requested : JetBrains Mono (pack resource)");
                lines.Add("resolved  : " + (names.Length == 0 ? "(nothing — fell back)" : names));
                GlyphTypeface? gt = null;
                foreach (var tf in Mono.GetTypefaces())
                {
                    if (tf.TryGetGlyphTypeface(out var g)) { gt = g; break; }
                }
                if (gt != null)
                {
                    var fam = "";
                    foreach (var v in gt.FamilyNames.Values) { fam = v; break; }
                    lines.Add("glyph face: " + fam);
                    foreach (var ch in new[] { '0', 'M', '─', '│', '┌', '○', '▶', '→' })
                    {
                        var has = gt.CharacterToGlyphMap.TryGetValue(ch, out var gi);
                        var adv = has ? gt.AdvanceWidths[gi] : -1;
                        lines.Add($"  U+{(int)ch:X4} '{ch}'  {(has ? adv.ToString("0.0000") + " em" : "MISSING")}");
                    }
                }
                else lines.Add("glyph face: could not be opened");

                // What WPF actually lays out, which is the only number that
                // decides whether a table lines up. The metrics above are what
                // the font claims; these are what the text stack does with it.
                lines.Add("");
                lines.Add("laid-out width of ten characters, at block size:");
                var probeFace = new Typeface(Mono, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal);
                foreach (var probe in new[] {
                    "0123456789", "MMMMMMMMMM", "          ",
                    "──────────", "││││││││││", "┌┐└┘├┤┬┴┼─",
                    "○○○○○○○○○○", "▶▶▶▶▶▶▶▶▶▶", "→→→→→→→→→→",
                    "│ Linear  ", "abcdefghij",
                    // The three rows from Tankun's screenshot, exactly.
                    "   ┌──────────┬────────────────┐",
                    "   │ Linear   │ pastes accepted│",
                    "   └──────────┴────────────────┘" })
                {
                    var ft = new FormattedText(probe, System.Globalization.CultureInfo.InvariantCulture,
                        FlowDirection.LeftToRight, probeFace, size - 1, System.Windows.Media.Brushes.Black, 1.0);
                    lines.Add($"  {ft.WidthIncludingTrailingWhitespace,8:0.000}   {probe}");
                }
                var dir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Collab");
                Directory.CreateDirectory(dir);
                File.WriteAllLines(Path.Combine(dir, "block-font.txt"), lines);
            }
            catch (Exception ex)
            {
                try
                {
                    var dir = Path.Combine(
                        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Collab");
                    File.WriteAllText(Path.Combine(dir, "block-font.txt"), "failed: " + ex.Message);
                }
                catch { }
            }
        }

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
            // The leading run of name characters, not the whole word with its
            // tail trimmed. Those differ on "@name's": trimming from the end
            // stops at the "s" and keeps the apostrophe, so the name came out
            // as "collab-build's" and never matched the reader's own name.
            int end = 0;
            while (end < n.Length && (char.IsLetterOrDigit(n[end]) || "-_./".IndexOf(n[end]) >= 0)) end++;
            while (end > 0 && (n[end - 1] == '.' || n[end - 1] == '/')) end--;
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

        /// How wide one cell's text lays out, with the inline markers that will
        /// not be drawn taken out first — a backtick or an asterisk is an
        /// instruction, not a character anybody sees.
        static double MeasureCell(string text, double size, bool header)
        {
            var plain = text.Replace("**", "").Replace("`", "");
            var face = new Typeface(docFamily ?? new FontFamily("Segoe UI"),
                                    FontStyles.Normal,
                                    header ? FontWeights.SemiBold : FontWeights.Normal,
                                    FontStretches.Normal);
            var ft = new FormattedText(plain, System.Globalization.CultureInfo.CurrentCulture,
                FlowDirection.LeftToRight, face, size, System.Windows.Media.Brushes.Black, 1.0);
            return ft.WidthIncludingTrailingWhitespace;
        }

        /// A markdown table as a real FlowDocument table, which sizes its own
        /// columns and stays selectable — the same reason a block is a paragraph
        /// here rather than a UI element in a box.
        static Table BuildTable(List<List<string>> rows, List<TextAlignment> align, double size)
        {
            int cols = align.Count;
            // The box the Mac draws around its tables. A Table is a Block, so it
            // carries the background and rule itself — no UI element wrapping
            // it, which would take the whole thing out of the selection model
            // the way the bordered code block did this morning.
            var t = new Table
            {
                CellSpacing = 0,
                Margin = new Thickness(0, 3, 0, 3),
                Padding = new Thickness(4),
                Background = Sol.BgAlt,
                BorderBrush = Sol.Rule,
                BorderThickness = new Thickness(1),
            };

            // Measured, not Auto.
            //
            // A FlowDocument table fills the page width and shares it out;
            // GridLength.Auto does not mean here what it means in a Grid, so the
            // columns stretched across the whole message list while the Mac's
            // sat at their content. Measuring each cell and setting an absolute
            // width is the only way to get the same shape — the leftover width
            // stays empty, which is what content-sized looks like.
            const double CellPad = 7;
            for (int c = 0; c < cols; c++)
            {
                double widest = 0;
                for (int r = 0; r < rows.Count; r++)
                {
                    if (c >= rows[r].Count) continue;
                    widest = Math.Max(widest, MeasureCell(rows[r][c], size, r == 0));
                }
                t.Columns.Add(new TableColumn { Width = new GridLength(widest + CellPad * 2 + 2) });
            }
            var group = new TableRowGroup();
            t.RowGroups.Add(group);

            for (int r = 0; r < rows.Count; r++)
            {
                var row = new TableRow();
                for (int c = 0; c < cols; c++)
                {
                    var text = c < rows[r].Count ? rows[r][c] : "";
                    var p = new Paragraph
                    {
                        Margin = new Thickness(0),
                        TextAlignment = align[c],
                        FontWeight = r == 0 ? FontWeights.SemiBold : FontWeights.Normal,
                    };
                    var made = new List<Inline>();
                    foreach (var seg in Split(text)) BuildSeg(seg, size, made);
                    foreach (var inl in made) p.Inlines.Add(inl);
                    row.Cells.Add(new TableCell(p)
                    {
                        Padding = new Thickness(7, 3, 7, 3),
                        Foreground = r == 0 ? Sol.FgEm : Sol.Fg,
                        // A rule under the header, which is what the separator
                        // line in the source is declaring it to be.
                        BorderBrush = Sol.Rule,
                        BorderThickness = new Thickness(0, 0, 0, r == 0 ? 1 : 0),
                    });
                }
                group.Rows.Add(row);
            }
            return t;
        }

        /// The cells of one `| a | b |` row. Leading and trailing pipes are
        /// optional, which is how people write them.
        static List<string> SplitRow(string line)
        {
            var t = line.Trim();
            if (t.StartsWith("|")) t = t.Substring(1);
            if (t.EndsWith("|")) t = t.Substring(0, t.Length - 1);
            var outp = new List<string>();
            foreach (var c in t.Split('|')) outp.Add(c.Trim());
            return outp;
        }

        /// A `|---|---|` row and the alignment it declares per column, or null
        /// if the line is not one. `:---` left, `---:` right, `:---:` centre.
        static List<TextAlignment>? SeparatorAlignments(string line)
        {
            var cells = SplitRow(line);
            if (cells.Count == 0) return null;
            var outp = new List<TextAlignment>();
            foreach (var raw in cells)
            {
                var c = raw.Trim();
                if (c.Length < 3 || !c.Contains("-")) return null;
                foreach (var ch in c) if (ch != '-' && ch != ':') return null;
                bool l = c.StartsWith(":"), r = c.EndsWith(":");
                outp.Add(l && r ? TextAlignment.Center : r ? TextAlignment.Right : TextAlignment.Left);
            }
            return outp;
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
                    var name = MentionName(text.Substring(i, end - i));
                    if (name.Length == 0) { buffer.Append(text[i]); i++; continue; }
                    // Colour the name, not the punctuation clinging to it.
                    var word = text.Substring(i, name.Length + 1);
                    end = i + word.Length;
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
                if (!LooksLikeEmoji(cluster, seg.Code)) { buffer.Append(cluster); continue; }

                // Rendered large, shown small.
                //
                // Noto is a bitmap font — its glyphs are 136px images — and
                // asking DirectWrite for them at text size draws them straight
                // down to about thirteen pixels, which loses most of them.
                // Segoe scaled cleanly because it is vector-ish; swapping the
                // font made the size the renderer was asked for start to matter.
                //
                // So it is drawn near the font's own size and scaled down by
                // WPF, which resamples rather than dropping pixels. One cache
                // entry per cluster now instead of one per cluster and size.
                const double DrawAt = 72;
                var key = (cluster, (int)DrawAt);
                if (!cache.TryGetValue(key, out var img))
                {
                    img = EmojiDW.Render(cluster, DrawAt);
                    cache[key] = img;
                }
                if (img == null) { buffer.Append(cluster); continue; }

                FlushText();
                // Sized and seated on the text baseline. The image is now
                // cropped to its ink, so its height is the glyph itself rather
                // than the font's padding — which is what let a Segoe-tuned
                // offset leave Noto's emoji floating above the line.
                //
                // A cap-height-ish box, dropped by a fifth so it sits on the
                // baseline rather than hanging from the ascender.
                // In a block, an emoji has to be exactly two cells wide.
                //
                // A monospace table lines up because every character advances
                // the same distance. An emoji sized by its own aspect ratio
                // advances by whatever it happens to be — so a row containing
                // one is wider than its neighbours and the columns shear. Every
                // character is present and the table is wrong, which is the
                // shape of thing that survives a long time unnoticed.
                //
                // JetBrains Mono advances 600/1000 em, measured from the font
                // rather than assumed, so two cells is 1.2 em of the block's own
                // size. The glyph is fitted inside that box rather than setting
                // it: a flag is wider than a face, and both must still advance
                // the same.
                double box = size * 1.2;
                var pic = new Image
                {
                    Source = img,
                    Height = box,
                    Stretch = Stretch.Uniform,
                    StretchDirection = StretchDirection.Both,
                    Margin = new Thickness(0, 0, 0, -size * 0.18),
                };
                if (seg.Code)
                {
                    double cell = (size - 1) * 0.6;
                    pic.Width = cell * 2;
                }
                // Resample on the way down rather than dropping pixels.
                RenderOptions.SetBitmapScalingMode(pic, BitmapScalingMode.HighQuality);
                made.Add(new InlineUIContainer(pic)
                { BaselineAlignment = BaselineAlignment.Baseline });
            }
            FlushText();
        }
    }
}
