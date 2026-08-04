using System.Linq;
using System.Threading;
using System.Windows.Forms;

namespace PulseMQTT;

internal static class Program
{
    [STAThread]
    private static void Main(string[] args)
    {
        Application.SetHighDpiMode(HighDpiMode.SystemAware);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        using var mutex = new Mutex(
            initiallyOwned: false,
            name: "PulseMQTT-SingleInstance-9F3B1C2A");

        // Nach einem Selbst-Update startet die neue EXE, während sich der
        // alte Prozess noch beendet – dann kurz auf die Mutex-Freigabe warten
        // statt sofort "läuft bereits" zu melden.
        var waitTime = args.Contains(UpdateService.RestartArgument)
            ? TimeSpan.FromSeconds(15)
            : TimeSpan.Zero;

        if (!TryAcquire(mutex, waitTime))
        {
            Localization.LanguageCode = AppSettings.Load().Language;
            MessageBox.Show(
                Localization.T("App.AlreadyRunning.Body"),
                "PulseMQTT", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        try
        {
            Application.Run(new TrayAppContext());
        }
        finally
        {
            mutex.ReleaseMutex();
        }
    }

    private static bool TryAcquire(Mutex mutex, TimeSpan timeout)
    {
        try
        {
            return mutex.WaitOne(timeout);
        }
        catch (AbandonedMutexException)
        {
            // Vorbesitzer wurde hart beendet – Mutex gehört jetzt uns.
            return true;
        }
    }
}
