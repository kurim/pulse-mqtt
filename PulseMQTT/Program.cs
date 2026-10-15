using System.Linq;
using System.Threading;
using System.Windows.Forms;

namespace PulseMQTT;

internal static class Program
{
    private static Mutex? _singleInstance;
    private static bool _owned;

    [STAThread]
    private static void Main(string[] args)
    {
        Application.SetHighDpiMode(HighDpiMode.SystemAware);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        _singleInstance = new Mutex(
            initiallyOwned: false,
            name: "PulseMQTT-SingleInstance-9F3B1C2A");

        // Nach einem Selbst-Update startet die neue EXE, während sich der
        // alte Prozess noch beendet – dann kurz auf die Mutex-Freigabe warten
        // statt sofort "läuft bereits" zu melden.
        var waitTime = args.Contains(UpdateService.RestartArgument)
            ? TimeSpan.FromSeconds(30)
            : TimeSpan.Zero;

        _owned = TryAcquire(_singleInstance, waitTime);
        if (!_owned)
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
            ReleaseSingleInstance();
            _singleInstance?.Dispose();
        }
    }

    /// <summary>
    /// Gibt den Single-Instance-Mutex frei. Das Selbst-Update ruft das auf,
    /// bevor es den neuen Prozess startet – sonst hängt die Freigabe daran,
    /// dass der alte Prozess vollständig aus Application.Run zurückkehrt.
    /// Muss auf dem Thread laufen, der den Mutex erworben hat (UI-Thread).
    /// </summary>
    internal static void ReleaseSingleInstance()
    {
        if (!_owned) return;
        _owned = false;
        try { _singleInstance?.ReleaseMutex(); } catch (ApplicationException) { }
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
