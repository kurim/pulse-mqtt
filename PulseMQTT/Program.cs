using System.Threading;
using System.Windows.Forms;

namespace PulseMQTT;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        Application.SetHighDpiMode(HighDpiMode.SystemAware);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        using var mutex = new Mutex(
            initiallyOwned: true,
            name: "PulseMQTT-SingleInstance-9F3B1C2A",
            createdNew: out bool isNew);

        if (!isNew)
        {
            Localization.LanguageCode = AppSettings.Load().Language;
            MessageBox.Show(
                Localization.T("App.AlreadyRunning.Body"),
                "PulseMQTT", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        Application.Run(new TrayAppContext());
    }
}
