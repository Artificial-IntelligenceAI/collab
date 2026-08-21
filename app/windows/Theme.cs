// Solarized, the same palette the Mac app uses. Light and dark share the eight
// accents; only the greys swap, which is the point of the scheme.
using System;
using System.Windows.Media;

namespace Collab
{
    public static class Sol
    {
        static SolidColorBrush B(byte r, byte g, byte b)
        {
            var s = new SolidColorBrush(Color.FromRgb(r, g, b));
            s.Freeze();
            return s;
        }

        public static readonly SolidColorBrush Base03 = B(0x00, 0x2b, 0x36);
        public static readonly SolidColorBrush Base02 = B(0x07, 0x36, 0x42);
        public static readonly SolidColorBrush Base01 = B(0x58, 0x6e, 0x75);
        public static readonly SolidColorBrush Base00 = B(0x65, 0x7b, 0x83);
        public static readonly SolidColorBrush Base0  = B(0x83, 0x94, 0x96);
        public static readonly SolidColorBrush Base1  = B(0x93, 0xa1, 0xa1);
        public static readonly SolidColorBrush Base2  = B(0xee, 0xe8, 0xd5);
        public static readonly SolidColorBrush Base3  = B(0xfd, 0xf6, 0xe3);

        public static readonly SolidColorBrush Yellow  = B(0xb5, 0x89, 0x00);
        public static readonly SolidColorBrush Orange  = B(0xcb, 0x4b, 0x16);
        public static readonly SolidColorBrush Red     = B(0xdc, 0x32, 0x2f);
        public static readonly SolidColorBrush Magenta = B(0xd3, 0x36, 0x82);
        public static readonly SolidColorBrush Violet  = B(0x6c, 0x71, 0xc4);
        public static readonly SolidColorBrush Blue    = B(0x26, 0x8b, 0xd2);
        public static readonly SolidColorBrush Cyan    = B(0x2a, 0xa1, 0x98);
        public static readonly SolidColorBrush Green   = B(0x85, 0x99, 0x00);

        /// Set once at startup and whenever Windows switches mode.
        public static bool Dark { get; set; } = true;

        public static SolidColorBrush Bg       => Dark ? Base03 : Base3;
        public static SolidColorBrush BgAlt    => Dark ? Base02 : Base2;
        public static SolidColorBrush Fg       => Dark ? Base0  : Base00;
        public static SolidColorBrush FgEm     => Dark ? Base1  : Base01;
        public static SolidColorBrush FgDim    => Dark ? Base01 : Base1;
        public static SolidColorBrush OnAccent => Dark ? Base03 : Base3;

        public static SolidColorBrush Rule
        {
            get
            {
                var c = (Dark ? Base1 : Base01).Color;
                var s = new SolidColorBrush(Color.FromArgb(Dark ? (byte)41 : (byte)46, c.R, c.G, c.B));
                s.Freeze();
                return s;
            }
        }

        /// One stable colour per person. The hash matches the Mac's exactly, so
        /// the same name is the same colour on both machines — otherwise two
        /// people looking at one conversation would be reading different
        /// pictures of it.
        public static SolidColorBrush ForName(string name)
        {
            var palette = new[] { Blue, Magenta, Cyan, Violet, Orange, Green, Yellow };
            uint h = 0;
            foreach (var b in System.Text.Encoding.UTF8.GetBytes(name)) h = unchecked(h * 31 + b);
            return palette[(int)(h % (uint)palette.Length)];
        }

        public static SolidColorBrush ForAction(string action) => action switch
        {
            "added"   => Green,
            "removed" => Red,
            "renamed" => Violet,
            _         => Blue,
        };

        /// Windows records the choice in the registry; there is no notification
        /// worth the plumbing, so this is read at startup and on window focus.
        public static bool SystemPrefersDark()
        {
            try
            {
                using var k = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(
                    @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
                return (k?.GetValue("AppsUseLightTheme") as int?) == 0;
            }
            catch { return true; }
        }
    }
}
