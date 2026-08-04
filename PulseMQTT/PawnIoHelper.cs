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
            Localization.T("PawnIo.NotFound.Body"),
            Localization.T("PawnIo.NotFound.Title"),
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
        // Zufälliger Dateiname + exklusives Anlegen (CreateNew): verhindert,
        // dass ein anderer Prozess die Datei unter einem vorhersagbaren Pfad
        // vorab platziert oder zwischen Download und msiexec-Start austauscht.
        var tempPath = Path.Combine(Path.GetTempPath(), $"PawnIO.Setup.{Guid.NewGuid():N}.msi");

        // Fortschritts-Ballon während des Downloads
        // (NotifyIcon ist zu diesem Zeitpunkt noch nicht übergeben, daher einfacher Dialog)
        using var progress = new ProgressForm(
            Localization.T("PawnIo.Downloading.Title"), Localization.T("PawnIo.Downloading.Body"));
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
            await using var file = new FileStream(tempPath, FileMode.CreateNew, FileAccess.Write, FileShare.None);
            await stream.CopyToAsync(file);
        }
        catch (Exception ex)
        {
            progress.Close();
            TryDelete(tempPath);
            MessageBox.Show(
                owner,
                Localization.T("PawnIo.DownloadFailed.Body", ex.Message),
                Localization.T("PawnIo.DownloadFailed.Title"),
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return false;
        }

        progress.Close();

        // Plausibilitätsprüfung vor dem Ausführen: MSI-Dateien sind
        // OLE-Compound-Files mit fester Magic-Byte-Signatur. Fängt u.a.
        // HTML-Fehlerseiten ab, die als Download durchgerutscht sind.
        if (!IsMsiFile(tempPath))
        {
            TryDelete(tempPath);
            MessageBox.Show(
                owner,
                Localization.T("PawnIo.DownloadFailed.Body", "Invalid installer file"),
                Localization.T("PawnIo.DownloadFailed.Title"),
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
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
                Localization.T("PawnIo.Installing.Body"),
                Localization.T("PawnIo.Installing.Title"),
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);

            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(
                owner,
                Localization.T("PawnIo.StartFailed.Body", ex.Message),
                Localization.T("Dialog.Error.Title"),
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return false;
        }
    }

    /// <summary>OLE-Compound-File-Signatur (D0 CF 11 E0 A1 B1 1A E1) prüfen.</summary>
    private static bool IsMsiFile(string path)
    {
        ReadOnlySpan<byte> magic = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        try
        {
            using var fs = File.OpenRead(path);
            Span<byte> header = stackalloc byte[8];
            return fs.Read(header) == header.Length && header.SequenceEqual(magic);
        }
        catch
        {
            return false;
        }
    }

    private static void TryDelete(string path)
    {
        try { File.Delete(path); } catch { /* Temp-Datei, wird ggf. vom OS geräumt */ }
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
