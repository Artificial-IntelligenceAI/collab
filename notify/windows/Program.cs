// collab-notify — posts one real Windows toast and exits.
//
// This is a separate C# program rather than part of the Go binary for the same
// reason the Mac side is a Swift .app: Windows attributes a toast to a
// registered AppUserModelID, and an unregistered process either gets no toast
// at all or one wearing somebody else's name. Registering means a Start Menu
// shortcut carrying the ID, plus a registry entry giving that ID a display
// name and an icon — both done here, on first run, and then left alone.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32;
using Windows.Data.Xml.Dom;
using Windows.UI.Notifications;

internal static class Program
{
    private const string Aumid   = "Tankun.Collab";
    private const string AppName = "collab";

    private static int Main(string[] args)
    {
        if (args.Length < 1)
        {
            Console.Error.WriteLine("usage: collab-notify <title> [body] [subtitle]");
            return 2;
        }
        string title     = args[0];
        string body      = args.Length > 1 ? args[1] : "";
        string subtitle  = args.Length > 2 ? args[2] : "";
        string windowUrl = args.Length > 3 ? args[3] : "";
        string collabExe = args.Length > 4 ? args[4] : "";
        string channel   = args.Length > 5 ? args[5] : "";
        string name      = args.Length > 6 ? args[6] : "";

        try
        {
            Register();
            Show(title, subtitle, body, ClickTarget(windowUrl, collabExe, channel, name));
            return 0;
        }
        catch (Exception e)
        {
            Console.Error.WriteLine("collab-notify: " + e.Message);
            return 5;
        }
    }

    // ── showing ────────────────────────────────────────────────

    // Where a click should go. Clicking a toast in an unpackaged app normally
    // needs a registered COM activator; protocol activation avoids all of that,
    // so collab registers a collab:// scheme pointing at `collab.exe gui` and
    // the toast just asks Windows to open it. Falling back to the plain http
    // address is worse — it shows a connection-refused page whenever the
    // window happens to be closed — so it is only used if collab.exe is not
    // where we expect it.
    private static string ClickTarget(string windowUrl, string collabExe, string channel, string name)
    {
        if (collabExe.Length > 0 && File.Exists(collabExe))
        {
            using var key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\collab");
            key.SetValue("", "URL:collab", RegistryValueKind.String);
            key.SetValue("URL Protocol", "", RegistryValueKind.String);
            using var cmd = key.CreateSubKey(@"shell\open\command");
            // %1 is the clicked URI; `collab gui -uri` reads the channel out of it,
            // so a click lands on the channel the message came from.
            cmd.SetValue("", "\"" + collabExe + "\" gui -uri \"%1\"", RegistryValueKind.String);
            return "collab://open?channel=" + Uri.EscapeDataString(channel)
                                    + "&name=" + Uri.EscapeDataString(name);
        }
        return channel.Length > 0
            ? windowUrl + "/?channel=" + Uri.EscapeDataString(channel)
            : windowUrl;
    }

    private static void Show(string title, string subtitle, string body, string launch)
    {
        var sb = new StringBuilder();
        sb.Append("<toast");
        if (launch.Length > 0)
            sb.Append(" activationType=\"protocol\" launch=\"").Append(Escape(launch)).Append('"');
        sb.Append("><visual><binding template=\"ToastGeneric\">");
        sb.Append("<text>").Append(Escape(title)).Append("</text>");
        if (subtitle.Length > 0) sb.Append("<text>").Append(Escape(subtitle)).Append("</text>");
        if (body.Length > 0)     sb.Append("<text>").Append(Escape(body)).Append("</text>");
        sb.Append("</binding></visual>");
        sb.Append("<audio src=\"ms-winsoundevent:Notification.Default\"/>");
        sb.Append("</toast>");

        var xml = new XmlDocument();
        xml.LoadXml(sb.ToString());
        ToastNotificationManager.CreateToastNotifier(Aumid).Show(new ToastNotification(xml));
    }

    private static string Escape(string s) =>
        s.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;").Replace("\"", "&quot;");

    // ── registration ───────────────────────────────────────────

    private static void Register()
    {
        string exe  = Environment.ProcessPath ?? AppContext.BaseDirectory;
        string dir  = Path.GetDirectoryName(exe) ?? AppContext.BaseDirectory;
        string icon = Path.Combine(dir, "collab.png");

        // Gives the ID a name and a picture, so the toast reads "collab".
        using (var key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\AppUserModelId\" + Aumid))
        {
            key.SetValue("DisplayName", AppName, RegistryValueKind.String);
            if (File.Exists(icon)) key.SetValue("IconUri", icon, RegistryValueKind.String);
        }

        // Windows only trusts an ID that a Start Menu shortcut vouches for.
        string link = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            @"Microsoft\Windows\Start Menu\Programs", AppName + ".lnk");
        if (!File.Exists(link)) CreateShortcut(link, exe);
    }

    private static void CreateShortcut(string path, string target)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var link = (IShellLinkW)new CShellLink();
        link.SetPath(target);
        link.SetArguments("");
        link.SetDescription("collab — messages from the other session");

        var store = (IPropertyStore)link;
        var key = new PropertyKey(new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), 5); // System.AppUserModel.ID
        IntPtr variant = IntPtr.Zero;
        try
        {
            Check(InitPropVariantFromStringAlloc(Aumid, out variant));
            store.SetValue(ref key, variant);
            store.Commit();
        }
        finally
        {
            if (variant != IntPtr.Zero) PropVariantClear(variant);
        }
        ((IPersistFile)link).Save(path, true);
    }

    private static void Check(int hr) { if (hr < 0) Marshal.ThrowExceptionForHR(hr); }

    // ── the COM bits Windows needs to write a shortcut ─────────

    [DllImport("propsys.dll", CharSet = CharSet.Unicode, PreserveSig = true)]
    private static extern int InitPropVariantFromStringAlloc(string psz, out IntPtr ppropvar);

    [DllImport("ole32.dll", PreserveSig = true)]
    private static extern int PropVariantClear(IntPtr pvar);

    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    private struct PropertyKey
    {
        private Guid fmtid;
        private int  pid;
        public PropertyKey(Guid id, int p) { fmtid = id; pid = p; }
    }

    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    private class CShellLink { }

    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder file, int maxLen, IntPtr fd, int flags);
        void GetIDList(out IntPtr ppidl);
        void SetIDList(IntPtr pidl);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder name, int maxLen);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string name);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder dir, int maxLen);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string dir);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder args, int maxLen);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string args);
        void GetHotkey(out short hotkey);
        void SetHotkey(short hotkey);
        void GetShowCmd(out int cmd);
        void SetShowCmd(int cmd);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder icon, int maxLen, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string icon, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pathRel, int reserved);
        void Resolve(IntPtr hwnd, int flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string file);
    }

    [ComImport, Guid("0000010B-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPersistFile
    {
        void GetClassID(out Guid clsid);
        [PreserveSig] int IsDirty();
        void Load([MarshalAs(UnmanagedType.LPWStr)] string file, uint mode);
        void Save([MarshalAs(UnmanagedType.LPWStr)] string file, [MarshalAs(UnmanagedType.Bool)] bool remember);
        void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string file);
        void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string file);
    }

    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        void GetCount(out uint count);
        void GetAt(uint index, out PropertyKey key);
        void GetValue(ref PropertyKey key, out IntPtr value);
        void SetValue(ref PropertyKey key, IntPtr value);
        void Commit();
    }
}
