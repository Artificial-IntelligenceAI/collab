// Colour emoji in WPF.
//
// WPF draws text through GlyphRuns and has no idea colour fonts exist, so an
// emoji comes out as the monochrome outline in Segoe UI Emoji. The colour is
// there in the font: COLR maps a base glyph to a list of layers, each layer a
// glyph plus an index into CPAL's palette. Drawing those layers in order, each
// in its own colour, is the whole trick — it is what every other renderer does.
//
// Reading two small tables is less risk than a dependency for a program that
// carries people's keys.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Collab
{
    public static class EmojiFont
    {
        static GlyphTypeface? face;
        /// Ligature rules from GSUB (lookup type 4), keyed by first glyph.
        ///
        /// This reaches skin tones — thumb + modifier really is one type-4 rule
        /// over the glyphs cmap gives you — and does not reach zero-width-joiner
        /// sequences. Segoe UI Emoji has no direct rule for man+ZWJ+woman: it
        /// uses types 1 and 6 as well, renaming each part to a joined variant
        /// under contextual rules before any ligature applies. Following that
        /// means running lookups in feature order with backtrack and lookahead
        /// matching — a shaping engine, and one whose subtle mistakes render the
        /// wrong emoji rather than an obviously broken one. A family therefore
        /// still comes out as its members. The correct fix is DirectWrite's
        /// shaper, not more of this by hand.
        static readonly Dictionary<ushort, List<(ushort[] rest, ushort result)>> ligatures = new();
        static readonly Dictionary<ushort, (int first, int count)> bases = new();
        static readonly List<(ushort glyph, ushort palette)> layers = new();
        static readonly List<Color> palette = new();
        static readonly Dictionary<(ushort glyph, int px), ImageSource?> cache = new();
        static bool tried;

        public static bool Available => Load() && bases.Count > 0;

        static bool Load()
        {
            if (tried) return face != null;
            tried = true;
            try
            {
                var path = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.Fonts), "seguiemj.ttf");
                if (!File.Exists(path)) return false;
                face = new GlyphTypeface(new Uri(path));
                var bytes = File.ReadAllBytes(path);
                ReadTables(bytes);
                return true;
            }
            catch { face = null; return false; }
        }

        // ── the two tables ─────────────────────────────────────

        static ushort U16(byte[] b, int i) => (ushort)((b[i] << 8) | b[i + 1]);
        static uint U32(byte[] b, int i) =>
            ((uint)b[i] << 24) | ((uint)b[i + 1] << 16) | ((uint)b[i + 2] << 8) | b[i + 3];

        static void ReadTables(byte[] b)
        {
            int numTables = U16(b, 4);
            int colr = -1, cpal = -1, gsub = -1;
            for (int i = 0; i < numTables; i++)
            {
                int rec = 12 + i * 16;
                var tag = System.Text.Encoding.ASCII.GetString(b, rec, 4);
                int off = (int)U32(b, rec + 8);
                if (tag == "COLR") colr = off;
                if (tag == "CPAL") cpal = off;
                if (tag == "GSUB") gsub = off;
            }
            if (gsub >= 0) { try { ReadGsub(b, gsub); } catch { ligatures.Clear(); } }
            if (colr < 0 || cpal < 0) return;

            // COLR v0: base records sorted by glyph id, each naming a run of layers.
            int numBase = U16(b, colr + 2);
            int baseOff = colr + (int)U32(b, colr + 4);
            int layerOff = colr + (int)U32(b, colr + 8);
            int numLayers = U16(b, colr + 12);
            for (int i = 0; i < numLayers; i++)
                layers.Add((U16(b, layerOff + i * 4), U16(b, layerOff + i * 4 + 2)));
            for (int i = 0; i < numBase; i++)
            {
                int r = baseOff + i * 6;
                bases[U16(b, r)] = (U16(b, r + 2), U16(b, r + 4));
            }

            // CPAL: BGRA colour records; the first palette is the default one.
            int numEntries = U16(b, cpal + 2);
            int recordsOff = cpal + (int)U32(b, cpal + 8);
            int firstIndex = U16(b, cpal + 12);
            for (int i = 0; i < numEntries; i++)
            {
                int r = recordsOff + (firstIndex + i) * 4;
                palette.Add(Color.FromArgb(b[r + 3], b[r + 2], b[r + 1], b[r]));
            }
        }

        /// Every ligature substitution in the font, taken from the lookup list
        /// directly rather than by walking scripts and features. Emoji fonts put
        /// these under ccmp, liga or rlig depending on the vendor, and the
        /// distinction does not matter here: any type-4 lookup describes a
        /// sequence that becomes one glyph, which is exactly what is needed.
        static void ReadGsub(byte[] b, int gsub)
        {
            int lookupList = gsub + U16(b, gsub + 8);
            int lookupCount = U16(b, lookupList);
            for (int i = 0; i < lookupCount; i++)
            {
                int lookup = lookupList + U16(b, lookupList + 2 + i * 2);
                int type = U16(b, lookup);
                int subCount = U16(b, lookup + 4);
                // Type 7 wraps another lookup so a table can exceed 16-bit offsets.
                for (int j = 0; j < subCount; j++)
                {
                    int sub = lookup + U16(b, lookup + 6 + j * 2);
                    int t = type;
                    if (t == 7)
                    {
                        t = U16(b, sub + 2);
                        sub = sub + (int)U32(b, sub + 4);
                    }
                    if (t == 4) ReadLigatureSubst(b, sub);
                }
            }
        }

        static void ReadLigatureSubst(byte[] b, int sub)
        {
            if (U16(b, sub) != 1) return;
            var first = ReadCoverage(b, sub + U16(b, sub + 2));
            int setCount = U16(b, sub + 4);
            for (int i = 0; i < setCount && i < first.Count; i++)
            {
                int set = sub + U16(b, sub + 6 + i * 2);
                int ligCount = U16(b, set);
                for (int k = 0; k < ligCount; k++)
                {
                    int lig = set + U16(b, set + 2 + k * 2);
                    ushort result = U16(b, lig);
                    int comps = U16(b, lig + 2);           // includes the first glyph
                    if (comps < 2) continue;
                    var rest = new ushort[comps - 1];
                    for (int c = 0; c < comps - 1; c++) rest[c] = U16(b, lig + 4 + c * 2);
                    if (!ligatures.TryGetValue(first[i], out var list))
                    {
                        list = new List<(ushort[], ushort)>();
                        ligatures[first[i]] = list;
                    }
                    list.Add((rest, result));
                }
            }
            // Longest first, so 👨‍👩‍👧 wins over 👨‍👩.
            foreach (var list in ligatures.Values) list.Sort((x, y) => y.rest.Length - x.rest.Length);
        }

        static List<ushort> ReadCoverage(byte[] b, int cov)
        {
            var outp = new List<ushort>();
            int format = U16(b, cov);
            if (format == 1)
            {
                int n = U16(b, cov + 2);
                for (int i = 0; i < n; i++) outp.Add(U16(b, cov + 4 + i * 2));
            }
            else if (format == 2)
            {
                int n = U16(b, cov + 2);
                for (int i = 0; i < n; i++)
                {
                    int r = cov + 4 + i * 6;
                    for (int g = U16(b, r); g <= U16(b, r + 2); g++) outp.Add((ushort)g);
                }
            }
            return outp;
        }

        /// The glyph for one codepoint, or null if the font has none.
        public static ushort? Glyph(int codepoint)
        {
            if (!Load() || face == null) return null;
            return face.CharacterToGlyphMap.TryGetValue(codepoint, out var g) ? g : (ushort?)null;
        }

        /// Applies the font's ligatures to a run of glyphs, longest match first.
        /// Returns how many glyphs were consumed and what they became.
        public static (ushort glyph, int used)? Ligate(IReadOnlyList<ushort> glyphs, int at)
        {
            if (!ligatures.TryGetValue(glyphs[at], out var rules)) return null;
            foreach (var (rest, result) in rules)
            {
                if (at + rest.Length > glyphs.Count - 1) continue; // not enough glyphs left
                bool ok = true;
                for (int i = 0; i < rest.Length; i++)
                {
                    if (glyphs[at + 1 + i] != rest[i]) { ok = false; break; }
                }
                if (ok) return (result, rest.Length + 1);
            }
            return null;
        }

        // ── drawing ────────────────────────────────────────────

        /// A picture of one emoji at one size, or null if the font has no
        /// colour form for it — in which case the caller lets WPF draw the text
        /// as it always did.
        public static ImageSource? Render(int codepoint, double px)
        {
            var g = Glyph(codepoint);
            return g is null ? null : RenderGlyph(g.Value, px);
        }

        /// By glyph, because a ligature produces one that no single codepoint
        /// maps to — there is no character for 👨‍👩‍👧, only the sequence.
        public static ImageSource? RenderGlyph(ushort glyph, double px)
        {
            var key = (glyph, (int)Math.Round(px));
            if (cache.TryGetValue(key, out var hit)) return hit;
            ImageSource? made = null;
            try { made = Draw(glyph, px); } catch { }
            cache[key] = made;
            return made;
        }

        public static bool HasColour(ushort glyph) => Load() && bases.ContainsKey(glyph);
        public static bool StartsLigature(ushort glyph) => Load() && ligatures.ContainsKey(glyph);

        static ImageSource? Draw(ushort glyph, double px)
        {
            if (!Load() || face == null) return null;
            if (!bases.TryGetValue(glyph, out var run)) return null;

            var group = new DrawingGroup();
            using (var dc = group.Open())
            {
                for (int i = 0; i < run.count; i++)
                {
                    var (g, pal) = layers[run.first + i];
                    var colour = pal == 0xFFFF || pal >= palette.Count ? Colors.Black : palette[pal];
                    var geo = face.GetGlyphOutline(g, px, px);
                    var brush = new SolidColorBrush(colour);
                    brush.Freeze();
                    dc.DrawGeometry(brush, null, geo);
                }
            }
            // Glyph outlines sit on the baseline, so shift them down into the box.
            var baseline = face.Baseline * px;
            var visual = new DrawingVisual();
            using (var dc = visual.RenderOpen())
            {
                dc.PushTransform(new TranslateTransform(0, baseline));
                dc.DrawDrawing(group);
                dc.Pop();
            }
            int w = (int)Math.Ceiling(px * 1.3), h = (int)Math.Ceiling(px * 1.3);
            var bmp = new RenderTargetBitmap(Math.Max(w, 1), Math.Max(h, 1), 96, 96, PixelFormats.Pbgra32);
            bmp.Render(visual);
            bmp.Freeze();
            return bmp;
        }
    }
}
