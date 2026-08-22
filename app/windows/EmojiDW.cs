// Colour emoji, rendered by Windows rather than by me.
//
// The previous version parsed the font's COLR and CPAL tables and drew the
// layers by hand. That reaches simple emoji and skin tones and stops there:
// 👨‍👩‍👧 is five codepoints the font turns into one glyph through contextual
// substitutions, and following those means implementing a shaping engine.
// Windows already has one. This hands the string to DirectWrite, draws it
// through Direct2D with colour fonts enabled, and reads back the pixels —
// which is what every correct renderer does, and gets every sequence, every
// flag and every script for free.
//
// Raw vtable calls rather than [ComImport] interfaces: only the handful of
// slots actually used are declared, so there is less to get wrong, and each
// one names the interface and index it belongs to.
using System;
using System.Runtime.InteropServices;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Collab
{
    internal static class EmojiDW
    {
        // ── plumbing ───────────────────────────────────────────

        [DllImport("d2d1.dll")]
        static extern int D2D1CreateFactory(int type, ref Guid riid, IntPtr options, out IntPtr factory);
        [DllImport("dwrite.dll")]
        static extern int DWriteCreateFactory(int type, ref Guid riid, out IntPtr factory);
        [DllImport("ole32.dll")]
        static extern int CoCreateInstance(ref Guid clsid, IntPtr outer, int ctx, ref Guid iid, out IntPtr obj);

        static Guid IID_ID2D1Factory      = new("06152247-6f50-465a-9245-118bfd3b6007");
        static Guid IID_IDWriteFactory    = new("b859ee5a-d838-4b5b-a2e8-1adc7d93db48");
        static Guid CLSID_WICImagingFactory = new("cacaf262-9370-4615-a13b-9f5539da4c0a");
        static Guid IID_IWICImagingFactory  = new("ec5ec8a9-c395-4314-9c77-54d7a935ff70");
        static Guid WICPixelFormat32bppPBGRA = new("6fddc324-4e03-4bfe-b185-3d77768dc910");

        /// The n'th function in an object's vtable, as a delegate.
        static T Slot<T>(IntPtr obj, int index) where T : Delegate
        {
            var vtable = Marshal.ReadIntPtr(obj);
            return Marshal.GetDelegateForFunctionPointer<T>(
                Marshal.ReadIntPtr(vtable, index * IntPtr.Size));
        }

        static void Release(IntPtr obj)
        {
            if (obj == IntPtr.Zero) return;
            Slot<ReleaseFn>(obj, 2)(obj);   // IUnknown::Release
        }

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate uint ReleaseFn(IntPtr self);

        // ── structs, laid out as the headers declare them ──────

        [StructLayout(LayoutKind.Sequential)] struct PixelFormat { public int Format; public int AlphaMode; }
        [StructLayout(LayoutKind.Sequential)]
        struct RenderTargetProperties
        {
            public int Type; public PixelFormat Pixel;
            public float DpiX, DpiY; public int Usage; public int MinLevel;
        }
        [StructLayout(LayoutKind.Sequential)] struct ColorF { public float R, G, B, A; }
        [StructLayout(LayoutKind.Sequential)] struct Point2F { public float X, Y; }
        [StructLayout(LayoutKind.Sequential)] struct WicRect { public int X, Y, Width, Height; }

        // ── the calls used ─────────────────────────────────────

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CreateBitmapFn(IntPtr self, uint w, uint h, ref Guid fmt, int cache, out IntPtr bmp);          // IWICImagingFactory slot 17
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CopyPixelsFn(IntPtr self, IntPtr rect, uint stride, uint bufSize, byte[] buf);                 // IWICBitmapSource slot 7
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CreateWicRTFn(IntPtr self, IntPtr wicBitmap, ref RenderTargetProperties props, out IntPtr rt); // ID2D1Factory slot 13
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CreateBrushFn(IntPtr self, ref ColorF colour, IntPtr brushProps, out IntPtr brush);            // ID2D1RenderTarget slot 8
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate void BeginDrawFn(IntPtr self);                                                                    // slot 48
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate void ClearFn(IntPtr self, ref ColorF colour);                                                     // slot 47
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate void DrawTextLayoutFn(IntPtr self, Point2F origin, IntPtr layout, IntPtr brush, int options);      // slot 28
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int EndDrawFn(IntPtr self, IntPtr tag1, IntPtr tag2);                                              // slot 49
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CreateTextFormatFn(IntPtr self, [MarshalAs(UnmanagedType.LPWStr)] string family,
            IntPtr collection, int weight, int style, int stretch, float size,
            [MarshalAs(UnmanagedType.LPWStr)] string locale, out IntPtr format);                                    // IDWriteFactory slot 15
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        delegate int CreateTextLayoutFn(IntPtr self, [MarshalAs(UnmanagedType.LPWStr)] string text, uint len,
            IntPtr format, float maxW, float maxH, out IntPtr layout);                                              // IDWriteFactory slot 18

        const int DRAW_TEXT_ENABLE_COLOR_FONT = 4;

        static IntPtr d2d, dwrite, wic;
        static bool tried, ok;

        static bool Init()
        {
            if (tried) return ok;
            tried = true;
            try
            {
                if (D2D1CreateFactory(0 /* single-threaded */, ref IID_ID2D1Factory, IntPtr.Zero, out d2d) != 0) return false;
                if (DWriteCreateFactory(0 /* shared */, ref IID_IDWriteFactory, out dwrite) != 0) return false;
                if (CoCreateInstance(ref CLSID_WICImagingFactory, IntPtr.Zero, 1 /* inproc */,
                                     ref IID_IWICImagingFactory, out wic) != 0) return false;
                ok = true;
            }
            catch { ok = false; }
            return ok;
        }

        public static bool Available => Init();

        /// Draws one string — normally a single emoji cluster — and returns it
        /// as an image, or null if anything at all went wrong. Nothing here is
        /// worth taking the window down for.
        public static ImageSource? Render(string text, double px)
        {
            if (!Init() || text.Length == 0) return null;
            IntPtr bmp = IntPtr.Zero, rt = IntPtr.Zero, brush = IntPtr.Zero,
                   format = IntPtr.Zero, layout = IntPtr.Zero;
            try
            {
                // Generous box: a cluster is never wider than a couple of ems,
                // and the transparent remainder costs nothing.
                uint side = (uint)Math.Ceiling(px * 2.0);
                if (Slot<CreateBitmapFn>(wic, 17)(wic, side, side, ref WICPixelFormat32bppPBGRA, 2, out bmp) != 0)
                    return null;

                var props = new RenderTargetProperties
                {
                    Type = 0,
                    Pixel = new PixelFormat { Format = 87 /* B8G8R8A8_UNORM */, AlphaMode = 1 /* premultiplied */ },
                    DpiX = 96, DpiY = 96, Usage = 0, MinLevel = 0,
                };
                if (Slot<CreateWicRTFn>(d2d, 13)(d2d, bmp, ref props, out rt) != 0) return null;

                if (Slot<CreateTextFormatFn>(dwrite, 15)(dwrite, "Segoe UI Emoji", IntPtr.Zero,
                        400 /* normal */, 0 /* normal */, 5 /* normal */, (float)px, "en-us", out format) != 0)
                    return null;
                if (Slot<CreateTextLayoutFn>(dwrite, 18)(dwrite, text, (uint)text.Length, format,
                        side, side, out layout) != 0)
                    return null;

                var black = new ColorF { R = 0, G = 0, B = 0, A = 1 };
                if (Slot<CreateBrushFn>(rt, 8)(rt, ref black, IntPtr.Zero, out brush) != 0) return null;

                var clear = new ColorF { R = 0, G = 0, B = 0, A = 0 };
                Slot<BeginDrawFn>(rt, 48)(rt);
                Slot<ClearFn>(rt, 47)(rt, ref clear);
                // The whole point: colour fonts enabled.
                Slot<DrawTextLayoutFn>(rt, 28)(rt, new Point2F { X = 0, Y = 0 }, layout, brush,
                                               DRAW_TEXT_ENABLE_COLOR_FONT);
                if (Slot<EndDrawFn>(rt, 49)(rt, IntPtr.Zero, IntPtr.Zero) != 0) return null;

                uint stride = side * 4;
                var buffer = new byte[stride * side];
                if (Slot<CopyPixelsFn>(bmp, 7)(bmp, IntPtr.Zero, stride, (uint)buffer.Length, buffer) != 0)
                    return null;

                var img = BitmapSource.Create((int)side, (int)side, 96, 96,
                    PixelFormats.Pbgra32, null, buffer, (int)stride);
                img.Freeze();
                return img;
            }
            catch { return null; }
            finally
            {
                Release(layout); Release(format); Release(brush); Release(rt); Release(bmp);
            }
        }
    }
}
