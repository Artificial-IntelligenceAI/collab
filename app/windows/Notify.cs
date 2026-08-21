// Toasts, folded into the app. On the Mac, Collab.app is the notifier; there is
// no separate helper, because Windows attributes a toast to a registered
// AppUserModelID and the app is the natural thing to register. Doing the same
// here removes collab-notify.exe entirely rather than shipping two programs
// that each carry a copy of the .NET runtime.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32;
using Windows.Data.Xml.Dom;
using Windows.UI.Notifications;

namespace Collab
{
internal static class Toast
{
    private const string Aumid   = "Tankun.Collab";
    private const string AppName = "collab";
    private static bool registered;

    /// Registration is idempotent and cheap after the first run, but there is
    /// no reason to repeat it per message.
    public static void Post(string title, string subtitle, string body)
    {
        try
        {
            if (!registered) { Register(); registered = true; }
            Show(title, subtitle, body, "");
        }
        catch { /* a missing toast must never take the window down with it */ }
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
            variant = MakeStringPropVariant(Aumid);
            store.SetValue(ref key, variant);
            store.Commit();
        }
        finally
        {
            if (variant != IntPtr.Zero)
            {
                PropVariantClear(variant);      // frees the string inside
                Marshal.FreeCoTaskMem(variant); // then the struct itself
            }
        }
        ((IPersistFile)link).Save(path, true);
    }

    private static void Check(int hr) { if (hr < 0) Marshal.ThrowExceptionForHR(hr); }

    // ── the COM bits Windows needs to write a shortcut ─────────

    /// A PROPVARIANT holding a string, built by hand.
    ///
    /// This used to call InitPropVariantFromStringAlloc in propsys.dll, which
    /// is not an export of it — the InitPropVariantFrom* helpers are inline
    /// functions in propvarutil.h. C# does not check that a DllImport resolves
    /// until it is called, so it compiled cleanly on the Mac and failed on
    /// Windows with "Entry point was not found", which is why notifications
    /// had never worked anywhere.
    ///
    /// Layout: VARTYPE at 0, three reserved words, then the pointer at the
    /// first pointer-aligned offset. PropVariantClear frees the string.
    private static IntPtr MakeStringPropVariant(string value)
    {
        const short VT_LPWSTR = 31;
        int size = IntPtr.Size == 8 ? 16 : 8;
        IntPtr pv = Marshal.AllocCoTaskMem(size);
        for (int i = 0; i < size; i++) Marshal.WriteByte(pv, i, 0);
        Marshal.WriteInt16(pv, 0, VT_LPWSTR);
        Marshal.WriteIntPtr(pv, IntPtr.Size, Marshal.StringToCoTaskMemUni(value));
        return pv;
    }

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
}
