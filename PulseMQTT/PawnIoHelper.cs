using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.ServiceProcess;
using Microsoft.Win32;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace PulseMQTT;

/// <summary>
/// Erkennt, ob der PawnIO-Treiber installiert ist, und bietet an,
/// ihn herunterzuladen und zu installieren.
///
/// PulseMQTT selbst bleibt dabei OHNE Adminrechte – der PawnIO-Installer
/// fordert UAC eigenständig an, da er einen Kernel-Treiber installiert.
/// </summary>
public static class PawnIoHelper
{
    // Offizieller Installer (MSI) aus dem GitHub-Release
    private const string InstallerUrl =
        "https://github.com/namazso/PawnIO.Setup/releases/latest/download/PawnIO.Setup.msi";

    private const string ServiceName = "PawnIO";

    /// <summary>
    /// Gibt true zurück, wenn der PawnIO-Treiber auf diesem System registriert ist.
    ///
    /// Erkennungs-Reihenfolge (von zuverlässig → Fallback):
    ///   1. Registry HKLM\SYSTEM\CurrentControlSet\Services\PawnIO
    ///      → funktioniert ohne Adminrechte, kein Exception-Risiko
    ///   2. ServiceController.Status
    ///      → kann bei eingeschränkten Rechten Exception werfen, daher Fallback
    ///   3. Treiberverzeichnis %ProgramFiles%\PawnIO
    /// </summary>
    public static bool IsInstalled()
    {
        // 1. Registry-Check – zuverlässigste Methode, kein Adminrecht nötig
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(
                @"SYSTEM\CurrentControlSet\Services\PawnIO");
            if (key is not null) return true;
        }
        catch { /* Registry nicht lesbar (sehr unwahrscheinlich) */ }

        // 2. ServiceController – kann bei fehlenden Rechten Exception werfen
        try
        {
            using var sc = new ServiceController(ServiceName);
            _ = sc.Status;
            return true;
        }
        catch (InvalidOperationException)
        {
            // Dienst definitiv nicht vorhanden
        }
        catch
        {
            // Zugriffsrechte-Problem: Dienst könnte trotzdem installiert sein.
            // Weiter zum Directory-Check.
        }

        // 3. Treiberverzeichnis als letzter Fallback
        var driverDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "PawnIO");
        return Directory.Exists(driverDir);
    }

    /// <summary>
    /// Zeigt einen Dialog und bietet Download+Installation an.
    /// Gibt true zurück, wenn der Installer gestartet wurde.
    /// </summary>
    public static async Task<bool> OfferInstallAsync(IWin32Window? owner = null)
    {
        var result = MessageBox.Show(
            owner,
            "Der PawnIO-Treiber ist nicht installiert.\n\n" +
            "PawnIO stellt den Low-Level-Hardwarezugriff für LibreHardwareMonitor bereit " +
            "(Nachfolger von WinRing0). Ohne ihn sind keine CPU-/GPU-Temperatur- und " +
            "Leistungswerte verfügbar – nur Last-Prozentwerte.\n\n" +
            "Soll PulseMQTT den PawnIO-Installer jetzt herunterladen und starten?\n" +
            "Windows fragt anschließend nach Administrator-Rechten " +
            "(der Installer benötigt sie für die Treiber-Installation).\n\n" +
            "Alternativ: Manuell von https://pawnio.eu/ herunterladen.",
            "PawnIO nicht gefunden – PulseMQTT",
            MessageBoxButtons.YesNo,
            MessageBoxIcon.Information,
            MessageBoxDefaultButton.Button1);

        if (result != DialogResult.Yes)
        {
            return false;
        }

        return await DownloadAndRunInstallerAsync(owner);
    }

    private static async Task<bool> DownloadAndRunInstallerAsync(IWin32Window? owner)
    {
        var tempPath = Path.Combine(Path.GetTempPath(), "PawnIO.Setup.msi");

        // Alten Download ggf. entfernen
        if (File.Exists(tempPath))
        {
            try { File.Delete(tempPath); } catch { }
        }

        // Fortschritts-Ballon während des Downloads
        // (NotifyIcon ist zu diesem Zeitpunkt noch nicht übergeben, daher einfacher Dialog)
        using var progress = new ProgressForm("PawnIO herunterladen…", "Lade Installer herunter...");
        progress.Show(owner);
        Application.DoEvents();

        try
        {
            using var http = new HttpClient();
            http.Timeout = TimeSpan.FromSeconds(60);

            // GitHub leitet von /latest/download/ weiter – HttpClient folgt Redirects.
            using var response = await http.GetAsync(InstallerUrl, HttpCompletionOption.ResponseHeadersRead);
            response.EnsureSuccessStatusCode();

            await using var stream = await response.Content.ReadAsStreamAsync();
            await using var file = File.Create(tempPath);
            await stream.CopyToAsync(file);
        }
        catch (Exception ex)
        {
            progress.Close();
            MessageBox.Show(
                owner,
                $"Download fehlgeschlagen:\n{ex.Message}\n\n" +
                "Bitte den Installer manuell von https://pawnio.eu/ herunterladen.",
                "Download-Fehler",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return false;
        }

        progress.Close();

        if (!File.Exists(tempPath))
        {
            return false;
        }

        // Installer starten – Windows zeigt automatisch die UAC-Abfrage,
        // weil der MSI-Installer Adminrechte benötigt. PulseMQTT selbst
        // eskaliert dabei NICHT.
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = "msiexec.exe",
                Arguments = $"/i \"{tempPath}\"",
                UseShellExecute = true   // nötig damit UAC erscheinen kann
            });

            MessageBox.Show(
                owner,
                "Der PawnIO-Installer wurde gestartet.\n\n" +
                "Nach der Installation bitte PulseMQTT neu starten, " +
                "damit der Treiber erkannt wird.",
                "PawnIO wird installiert",
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);

            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(
                owner,
                $"Installer konnte nicht gestartet werden:\n{ex.Message}",
                "Fehler",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return false;
        }
    }
}

/// <summary>
/// Einfaches modales Fortschrittsfenster ohne Statusbalken
/// (nur Beschriftung, damit die App nicht einfriert).
/// </summary>
internal sealed class ProgressForm : Form
{
    public ProgressForm(string title, string message)
    {
        Text = title;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        ControlBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new System.Drawing.Size(320, 70);
        ShowInTaskbar = false;

        Controls.Add(new Label
        {
            Text = message,
            AutoSize = false,
            Dock = DockStyle.Fill,
            TextAlign = System.Drawing.ContentAlignment.MiddleCenter
        });
    }
}
