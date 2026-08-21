using System;
using System.Windows;

namespace Collab
{
    public partial class App : Application
    {
        protected override void OnStartup(StartupEventArgs e)
        {
            Sol.Dark = Sol.SystemPrefersDark();

            // Raise one toast and leave, without ever showing a window. This is
            // how the command line gets a notification on Windows: it asks the
            // app, the way `collab` on the Mac asks Collab.app. It means there
            // is one program that owns notifications rather than a second
            // 94 MB helper carrying its own copy of the runtime purely to say
            // the same sentence.
            //   Collab.exe --toast <title> [body] [subtitle]
            if (e.Args.Length >= 2 && e.Args[0] == "--toast")
            {
                var title = e.Args[1];
                var body = e.Args.Length > 2 ? e.Args[2] : "";
                var subtitle = e.Args.Length > 3 ? e.Args[3] : "";
                try { Toast.Post(title, subtitle, body); }
                catch (Exception ex) { Console.Error.WriteLine("collab: " + ex.Message); Environment.Exit(5); }
                Shutdown(0);
                return;
            }

            base.OnStartup(e);
        }
    }
}
