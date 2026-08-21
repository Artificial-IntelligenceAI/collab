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
        static readonly Dictionary<ushort, (int first, int count)> bases = new();
        static readonly List<(ushort glyph, ushort palette)> layers = new();
        static readonly List<Color> palette = new();
        static readonly Dictionary<(int cp, int px), ImageSource?> cache = new();
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
            int colr = -1, cpal = -1;
            for (int i = 0; i < numTables; i++)
            {
                int rec = 12 + i * 16;
                var tag = System.Text.Encoding.ASCII.GetString(b, rec, 4);
                int off = (int)U32(b, rec + 8);
                if (tag == "COLR") colr = off;
                if (tag == "CPAL") cpal = off;
            }
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

        // ── drawing ────────────────────────────────────────────

        /// A picture of one emoji at one size, or null if the font has no
        /// colour form for it — in which case the caller lets WPF draw the text
        /// as it always did.
        public static ImageSource? Render(int codepoint, double px)
        {
            var key = (codepoint, (int)Math.Round(px));
            if (cache.TryGetValue(key, out var hit)) return hit;
            ImageSource? made = null;
            try { made = Draw(codepoint, px); } catch { }
            cache[key] = made;
            return made;
        }

        static ImageSource? Draw(int codepoint, double px)
        {
            if (!Load() || face == null) return null;
            if (!face.CharacterToGlyphMap.TryGetValue(codepoint, out var glyph)) return null;
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
