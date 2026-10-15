using System;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace PulseMQTT;

/// <summary>Beschreibt ein verfügbares Update aus dem neuesten GitHub-Release.</summary>
public sealed record UpdateInfo(string TagName, string AssetName, string DownloadUrl);

/// <summary>
/// Selbst-Update über GitHub-Releases: fragt
/// https://github.com/kurim/pulse-mqtt/releases/latest ab, lädt das passende
/// Release-ZIP herunter, entpackt die neue PulseMQTT.exe und tauscht sie
/// gegen die laufende Datei aus. Windows erlaubt das Umbenennen einer
/// laufenden EXE (nur kein Überschreiben), daher:
///
///   1. PulseMQTT.exe  →  PulseMQTT.exe.old   (laufende Datei wegbenennen)
///   2. neue EXE       →  PulseMQTT.exe       (an den alten Platz schieben)
///   3. alter Prozess fährt herunter und gibt den Single-Instance-Mutex
///      frei, startet dann den neuen Prozess und beendet sich;
///      der neue räumt die .old-Datei beim Start weg.
/// </summary>
public static class UpdateService
{
    private const string LatestReleaseApi =
        "https://api.github.com/repos/kurim/pulse-mqtt/releases/latest";

    /// <summary>
    /// Kommandozeilen-Flag, mit dem der neue Prozess nach einem Update
    /// gestartet wird. Program.Main wartet damit kurz auf die Freigabe des
    /// Single-Instance-Mutex durch den sich gerade beendenden alten Prozess.
    /// </summary>
    public const string RestartArgument = "--updated";

    /// <summary>Installierte Version, z.B. "0.1.0" oder "0.1.0-beta".</summary>
    public static string CurrentVersionDisplay
    {
        get
        {
            var info = Assembly.GetExecutingAssembly()
                .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
                .InformationalVersion;
            if (string.IsNullOrWhiteSpace(info))
                info = Application.ProductVersion;

            // Build-Metadaten ("+<commit>") abschneiden
            var plus = info.IndexOf('+');
            return plus >= 0 ? info[..plus] : info;
        }
    }

    /// <summary>
    /// Prüft das neueste GitHub-Release. Gibt null zurück wenn bereits die
    /// aktuellste Version läuft, sonst die Download-Infos. Wirft bei
    /// Netzwerk-/Parsing-Fehlern.
    /// </summary>
    public static async Task<UpdateInfo?> CheckForUpdateAsync()
    {
        using var http = CreateClient(TimeSpan.FromSeconds(30));
        var json = await http.GetStringAsync(LatestReleaseApi);

        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;

        var tag = root.GetProperty("tag_name").GetString()
            ?? throw new InvalidDataException("Release without tag_name");

        var remote = ParseVersion(tag)
            ?? throw new InvalidDataException($"Unparsable release tag \"{tag}\"");

        if (!IsNewer(remote)) return null;

        // Passendes ZIP wählen: self-contained ("-win-x64.zip") bzw.
        // framework-dependent ("-win-x64-net.zip") – je nachdem, welche
        // Variante gerade läuft. Fallback ist das self-contained ZIP,
        // das läuft ohne installierte .NET-Runtime überall.
        var assets = root.GetProperty("assets").EnumerateArray()
            .Select(a => (
                Name: a.GetProperty("name").GetString() ?? "",
                Url:  a.GetProperty("browser_download_url").GetString() ?? ""))
            .Where(a => a.Name.Length > 0 && a.Url.Length > 0)
            .ToList();

        var suffix = IsSelfContained() ? "-win-x64.zip" : "-win-x64-net.zip";
        var asset = assets.FirstOrDefault(a =>
                a.Name.EndsWith(suffix, StringComparison.OrdinalIgnoreCase));
        if (string.IsNullOrEmpty(asset.Name))
            asset = assets.FirstOrDefault(a =>
                a.Name.EndsWith("-win-x64.zip", StringComparison.OrdinalIgnoreCase));
        if (string.IsNullOrEmpty(asset.Name))
            throw new InvalidDataException("Release contains no win-x64 ZIP asset");

        return new UpdateInfo(tag, asset.Name, asset.Url);
    }

    /// <summary>
    /// Lädt das Update-ZIP in ein Temp-Verzeichnis, prüft es und entpackt
    /// die neue PulseMQTT.exe. Gibt den Pfad der entpackten EXE zurück.
    /// </summary>
    public static async Task<string> DownloadAsync(UpdateInfo update)
    {
        var tempDir = Path.Combine(Path.GetTempPath(), $"PulseMQTT-Update-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDir);
        var zipPath = Path.Combine(tempDir, update.AssetName);

        try
        {
            // 10 Minuten Timeout: das self-contained ZIP ist ~70 MB und muss
            // auch über langsame Leitungen ankommen.
            using (var http = CreateClient(TimeSpan.FromMinutes(10)))
            using (var response = await http.GetAsync(
                update.DownloadUrl, HttpCompletionOption.ResponseHeadersRead))
            {
                response.EnsureSuccessStatusCode();
                await using var stream = await response.Content.ReadAsStreamAsync();
                await using var file = new FileStream(
                    zipPath, FileMode.CreateNew, FileAccess.Write, FileShare.None);
                await stream.CopyToAsync(file);
            }

            // ZIP-Signatur prüfen – fängt HTML-Fehlerseiten ab, die als
            // Download durchgerutscht sind.
            if (!HasMagic(zipPath, [0x50, 0x4B, 0x03, 0x04]))
                throw new InvalidDataException("Downloaded file is not a ZIP archive");

            var exePath = Path.Combine(tempDir, "PulseMQTT.exe");
            using (var archive = ZipFile.OpenRead(zipPath))
            {
                var entry = archive.Entries.FirstOrDefault(e =>
                        e.Name.Equals("PulseMQTT.exe", StringComparison.OrdinalIgnoreCase))
                    ?? throw new InvalidDataException("ZIP contains no PulseMQTT.exe");
                entry.ExtractToFile(exePath, overwrite: true);
            }
            TryDelete(zipPath);

            // PE-Signatur ("MZ") der entpackten EXE prüfen
            if (!HasMagic(exePath, [0x4D, 0x5A]))
                throw new InvalidDataException("Extracted PulseMQTT.exe is not a valid executable");

            return exePath;
        }
        catch
        {
            try { Directory.Delete(tempDir, recursive: true); } catch { }
            throw;
        }
    }

    /// <summary>
    /// Tauscht die laufende EXE gegen die neue aus (.old-Backup). Startet noch
    /// nichts: der Aufrufer fährt erst Broker/Ports herunter und ruft dann
    /// <see cref="StartUpdatedInstance"/>. Bei einem Fehler wird die alte EXE
    /// wiederhergestellt und die Exception weitergereicht.
    /// </summary>
    public static void Apply(string stagedExePath)
    {
        var exePath = Environment.ProcessPath ?? Application.ExecutablePath;
        var backupPath = exePath + ".old";

        TryDelete(backupPath);
        File.Move(exePath, backupPath);
        try
        {
            File.Move(stagedExePath, exePath);
        }
        catch
        {
            // Rollback: alte EXE zurück an ihren Platz
            try
            {
                TryDelete(exePath);
                File.Move(backupPath, exePath);
            }
            catch { }
            throw;
        }

        try { Directory.Delete(Path.GetDirectoryName(stagedExePath)!, recursive: true); } catch { }
    }

    /// <summary>
    /// Startet die ausgetauschte EXE. Muss nach dem Herunterfahren und nach
    /// <c>Program.ReleaseSingleInstance()</c> aufgerufen werden, damit der
    /// neue Prozess den Mutex sofort bekommt.
    /// </summary>
    public static void StartUpdatedInstance()
    {
        var exePath = Environment.ProcessPath ?? Application.ExecutablePath;

        // Ein aus einem Admin-Prozess gestarteter Kindprozess erbt die
        // Elevation – die App startet also mit denselben Rechten wie zuvor.
        Process.Start(new ProcessStartInfo
        {
            FileName        = exePath,
            Arguments       = RestartArgument,
            UseShellExecute = true
        });
    }

    /// <summary>
    /// Räumt die vom letzten Update übrig gebliebene "PulseMQTT.exe.old"
    /// weg. Läuft im Hintergrund mit Wiederholungen, weil der alte Prozess
    /// beim Start des neuen evtl. noch nicht vollständig beendet ist.
    /// </summary>
    public static void CleanupAfterUpdate()
    {
        var backupPath = (Environment.ProcessPath ?? Application.ExecutablePath) + ".old";
        if (!File.Exists(backupPath)) return;

        _ = Task.Run(async () =>
        {
            for (var attempt = 0; attempt < 10; attempt++)
            {
                try { File.Delete(backupPath); return; }
                catch { await Task.Delay(1000); }
            }
        });
    }

    // ── Intern ───────────────────────────────────────────────────────────────

    /// <summary>Remote-Version gegen die laufende Version vergleichen.</summary>
    private static bool IsNewer(Version remote)
    {
        var display = CurrentVersionDisplay;                  // z.B. "0.1.0-beta"
        var local = ParseVersion(display) ?? new Version(0, 0, 0, 0);
        var localIsPrerelease = display.Contains('-');

        if (remote > local) return true;

        // /releases/latest liefert nie Prereleases – bei gleicher Nummer ist
        // das Release also die stabile Variante einer lokalen Beta.
        return remote == local && localIsPrerelease;
    }

    /// <summary>"v0.1.0-beta" → Version 0.1.0.0 (Suffix und "v" entfernt).</summary>
    private static Version? ParseVersion(string tag)
    {
        var s = tag.Trim().TrimStart('v', 'V');
        var dash = s.IndexOf('-');
        if (dash >= 0) s = s[..dash];
        if (!s.Contains('.')) s += ".0";

        return Version.TryParse(s, out var v)
            ? new Version(v.Major, Math.Max(v.Minor, 0), Math.Max(v.Build, 0), Math.Max(v.Revision, 0))
            : null;
    }

    /// <summary>
    /// Läuft gerade die self-contained Variante (Runtime eingebettet) oder
    /// die framework-dependent Variante (nutzt die installierte .NET-Runtime)?
    /// Entscheidet, welches Release-ZIP heruntergeladen wird.
    /// </summary>
    private static bool IsSelfContained()
    {
        try
        {
            // Framework-dependent Apps lösen ihre Runtime über das global
            // installierte "Microsoft.NETCore.App" Shared Framework auf.
            if (AppContext.GetData("FX_DEPS_FILE") is string fxDeps && fxDeps.Length > 0)
                return !fxDeps.Contains("Microsoft.NETCore.App", StringComparison.OrdinalIgnoreCase);
        }
        catch { }

        try
        {
            // Fallback über die Dateigröße: self-contained ~70 MB,
            // framework-dependent ~8 MB.
            var exePath = Environment.ProcessPath ?? Application.ExecutablePath;
            return new FileInfo(exePath).Length > 25L * 1024 * 1024;
        }
        catch
        {
            return true; // self-contained ZIP läuft im Zweifel überall
        }
    }

    private static HttpClient CreateClient(TimeSpan timeout)
    {
        var http = new HttpClient { Timeout = timeout };
        // Die GitHub-API verlangt einen User-Agent
        http.DefaultRequestHeaders.UserAgent.ParseAdd("PulseMQTT");
        http.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        return http;
    }

    private static bool HasMagic(string path, ReadOnlySpan<byte> magic)
    {
        try
        {
            using var fs = File.OpenRead(path);
            Span<byte> header = stackalloc byte[8];
            var read = fs.Read(header[..magic.Length]);
            return read == magic.Length && header[..magic.Length].SequenceEqual(magic);
        }
        catch
        {
            return false;
        }
    }

    private static void TryDelete(string path)
    {
        try { if (File.Exists(path)) File.Delete(path); } catch { }
    }
}
