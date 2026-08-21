using System.Windows;

namespace Collab
{
    public partial class App : Application
    {
        protected override void OnStartup(StartupEventArgs e)
        {
            Sol.Dark = Sol.SystemPrefersDark();
            base.OnStartup(e);
        }
    }
}
