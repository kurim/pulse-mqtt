using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows.Forms;
using Microsoft.Win32;

namespace PulseMQTT;

public sealed class TrayAppContext : ApplicationContext
{
    private readonly NotifyIcon _trayIcon;
    private readonly System.Windows.Forms.Timer _pollTimer;
    private readonly System.Windows.Forms.Timer _memoryTrimTimer;
    private readonly System.Windows.Forms.Timer _updateTimer;
    private readonly MqttBrokerService _broker = new();
    private readonly SerialBridgeService _serial = new();
    private readonly HardwareMonitorService _hardware = new();
    private readonly Dictionary<string, float> _payloadBuffer = new(StringComparer.Ordinal);
    private AppSettings _settings;
    private bool _accessWarningShown;
    private bool _isPublishing;
    private bool _updateInProgress;
    private DateTime _nextSerialRetryUtc = DateTime.MinValue;
    private static readonly TimeSpan SerialRetryInterval = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan MemoryTrimInterval = TimeSpan.FromMinutes(5);
    private static readonly TimeSpan UpdateCheckInterval = TimeSpan.FromHours(24);

    private static readonly bool IsAdmin =
        new WindowsPrincipal(WindowsIdentity.GetCurrent())
            .IsInRole(WindowsBuiltInRole.Administrator);

    public TrayAppContext()
    {
        _settings = AppSettings.Load();
        Localization.LanguageCode = _settings.Language;

        _trayIcon = new NotifyIcon
        {
            Icon             = AppIcon.Load(),
            Text             = Localization.T("Tray.Starting", "PulseMQTT"),
            ContextMenuStrip = BuildContextMenu(),
            Visible          = true
        };
        _trayIcon.DoubleClick += OnSensorsClicked;

        _pollTimer = new System.Windows.Forms.Timer();
        _pollTimer.Tick += async (_, _) => await PublishSnapshotAsync();

        // Regelmäßig den Heap kompaktieren und das Working Set an das OS
        // zurückgeben. Ohne diesen Trim behält die CLR einmal committeten
        // Speicher (JIT-Warmup, Sensor-Discovery, Dialoge) dauerhaft –
        // das ist der Hauptgrund für 150+ MB im Task-Manager.
        _memoryTrimTimer = new System.Windows.Forms.Timer
            { Interval = (int)MemoryTrimInterval.TotalMilliseconds };
        _memoryTrimTimer.Tick += (_, _) => TrimMemory();
        _memoryTrimTimer.Start();

        // Täglich still auf eine neue Version prüfen (zusätzlich zum
        // Check beim Start), sofern automatische Updates aktiviert sind.
        _updateTimer = new System.Windows.Forms.Timer
            { Interval = (int)UpdateCheckInterval.TotalMilliseconds };
        _updateTimer.Tick += async (_, _) =>
        {
            if (_settings.AutoUpdate) await CheckForUpdateAsync(interactive: false);
        };
        _updateTimer.Start();

        _ = StartupAsync();
    }

    private ContextMenuStrip BuildContextMenu()
    {
        var menu = new ContextMenuStrip();
        menu.Items.Add(Localization.T("Menu.Settings"),      null, OnSettingsClicked);
        menu.Items.Add(Localization.T("Menu.SelectSensors"), null, OnSensorsClicked);
        menu.Items.Add(new ToolStripSeparator());

        if (!IsAdmin)
            menu.Items.Add(Localization.T("Menu.RestartAsAdmin"), null, OnRestartAsAdminClicked);
        else
        {
            var adminItem = new ToolStripMenuItem(Localization.T("Menu.RunningAsAdmin")) { Enabled = false };
            menu.Items.Add(adminItem);
        }

        if (PawnIoHelper.IsInstalled())
        {
            menu.Items.Add(new ToolStripMenuItem(Localization.T("Menu.PawnIoInstalled")) { Enabled = false });
        }
        else
        {
            menu.Items.Add(Localization.T("Menu.InstallPawnIo"), null, OnInstallPawnIoClicked);
        }
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(Localization.T("Menu.CheckUpdate", UpdateService.CurrentVersionDisplay),
            null, OnCheckUpdateClicked);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(Localization.T("Menu.Exit"), null, OnExitClicked);

        return menu;
    }

    private async Task StartupAsync()
    {
        // Nach einem Selbst-Update liegt noch die alte EXE als
        // "PulseMQTT.exe.old" daneben – im Hintergrund wegräumen.
        UpdateService.CleanupAfterUpdate();

        await ApplySettingsAsync(_settings);

        // Sensor-Erstkonfiguration wenn noch nichts ausgewählt wurde
        if (_settings.EnabledSensors.Count == 0)
            await AutoConfigureSensorsAsync();

        if (!PawnIoHelper.IsInstalled())
            await PawnIoHelper.OfferInstallAsync();

        if (_settings.AutoUpdate)
        {
            await Task.Delay(5000); // App erst in Ruhe hochfahren lassen
            await CheckForUpdateAsync(interactive: false);
        }
    }

    /// <summary>
    /// Beim ersten Start: Sensoren entdecken, Standardwerte vorauswählen,
    /// dann den Picker öffnen damit der Benutzer anpassen kann.
    /// </summary>
    private async Task AutoConfigureSensorsAsync()
    {
        await Task.Delay(300); // kurz warten bis das Tray-Icon fertig ist

        var sensors  = _hardware.DiscoverSensors();
        var defaults = HardwareMonitorService.BuildDefaultSelection(sensors);
        _settings.EnabledSensors = defaults;

        using var picker = new SensorPickerForm(sensors, defaults, _hardware.GetValues);
        var result = picker.ShowDialog();
        if (result == DialogResult.OK)
            _settings.EnabledSensors = picker.Result;

        _settings.Save();
    }

    private async Task ApplySettingsAsync(AppSettings settings)
    {
        _settings = settings;
        Localization.LanguageCode = settings.Language;

        // Altes Menü freigeben – ContextMenuStrip hält Win32-Handles, die
        // sonst bei jedem Speichern der Einstellungen liegenbleiben.
        var oldMenu = _trayIcon.ContextMenuStrip;
        _trayIcon.ContextMenuStrip = BuildContextMenu();
        oldMenu?.Dispose();

        _pollTimer.Stop();
        _pollTimer.Interval = Math.Max(250, (int)(settings.UpdateIntervalSeconds * 1000));

        if (settings.UseMqtt)
        {
            try
            {
                await _broker.StartAsync(settings.MqttPort, settings.MqttUsername, settings.MqttPassword);
                _trayIcon.Text = Truncate(Localization.T("Tray.PortTopic", "PulseMQTT", settings.MqttPort, settings.MqttTopic));
            }
            catch (Exception ex)
            {
                _trayIcon.Text = Truncate(Localization.T("Tray.MqttError", "PulseMQTT", ex.Message));
                _trayIcon.ShowBalloonTip(5000, "PulseMQTT",
                    Localization.T("Balloon.MqttStartFailed.Body", settings.MqttPort, ex.Message),
                    ToolTipIcon.Error);
            }
        }
        else
        {
            await _broker.StopAsync();
        }

        if (settings.UseSerial && !string.IsNullOrWhiteSpace(settings.SerialPortName))
        {
            try
            {
                await _serial.OpenAsync(settings.SerialPortName, settings.SerialMode);
            }
            catch (Exception ex)
            {
                _trayIcon.ShowBalloonTip(5000, "PulseMQTT",
                    Localization.T("Balloon.SerialStartFailed.Body", settings.SerialPortName, ex.Message),
                    ToolTipIcon.Error);
            }
        }
        else
        {
            await _serial.CloseAsync();
        }

        SetAutoStart(settings.StartWithWindows);
        _pollTimer.Start();
    }

    private async Task PublishSnapshotAsync()
    {
        if (_isPublishing) return;
        _isPublishing = true;
        try
        {
            var values = _hardware.GetValues();

            // Payload dynamisch aus gewählten Sensoren aufbauen
            // (wiederverwendeter Puffer statt neuem Dictionary pro Tick)
            var payload = _payloadBuffer;
            payload.Clear();
            var enabledCount = 0;
            var hasData = false;

            foreach (var entry in _settings.EnabledSensors)
            {
                if (!entry.IsEnabled) continue;
                enabledCount++;

                if (values.TryGetValue(entry.Identifier, out var val))
                {
                    payload[entry.MqttKey] = MathF.Round(val, 1);
                    if (val > 0) hasData = true;
                }
            }

            if (!_accessWarningShown && enabledCount > 0 &&
                !hasData && !IsAdmin && PawnIoHelper.IsInstalled())
            {
                _accessWarningShown = true;
                _trayIcon.ShowBalloonTip(10_000,
                    Localization.T("Balloon.RestrictedAccess.Title"),
                    Localization.T("Balloon.RestrictedAccess.Body"),
                    ToolTipIcon.Warning);
            }

            var json = JsonSerializer.Serialize(payload);

            if (_settings.UseMqtt)
                await _broker.PublishAsync(_settings.MqttTopic, json);
            if (_settings.UseSerial)
                await EnsureSerialConnectedAsync();
            if (_settings.UseSerial && _serial.IsOpen)
                await _serial.WriteLineAsync(json);

            // Tooltip: die ersten 3-4 Werte anzeigen
            var preview = string.Join("  │  ", payload
                .Take(4)
                .Select(kv => $"{kv.Key}: {kv.Value}"));

            var source = BuildSourceLabel();
            _trayIcon.Text = Truncate(Localization.T("Tray.Status",
                "PulseMQTT", IsAdmin ? " 🔒" : "", source, preview));
        }
        catch (Exception ex)
        {
            _trayIcon.Text = Truncate(Localization.T("Tray.Error", "PulseMQTT", ex.Message));
        }
        finally
        {
            _isPublishing = false;
        }
    }

    /// <summary>
    /// Versucht die serielle Verbindung wiederherzustellen, falls sie (z.B.
    /// nach einem Board-Reset durch DTR oder einem Schreibfehler) getrennt
    /// wurde. Läuft mit Cooldown, damit nicht jeder Poll-Tick einen neuen
    /// Verbindungsversuch (inkl. Timeout) auslöst.
    /// </summary>
    private async Task EnsureSerialConnectedAsync()
    {
        if (_serial.IsOpen || DateTime.UtcNow < _nextSerialRetryUtc)
            return;

        _nextSerialRetryUtc = DateTime.UtcNow + SerialRetryInterval;
        try
        {
            await _serial.OpenAsync(_settings.SerialPortName, _settings.SerialMode);
        }
        catch { /* nächster Versuch nach Cooldown */ }
    }

    private string BuildSourceLabel()
    {
        var parts = new List<string>();
        if (_settings.UseMqtt)
            parts.Add(Localization.T("Tray.Source.Mqtt", _broker.ConnectedClients, _settings.MqttPort));
        if (_settings.UseSerial)
            parts.Add(_serial.IsOpen
                ? Localization.T("Tray.Source.Serial", _settings.SerialPortName)
                : Localization.T("Tray.Source.SerialDisconnected", _settings.SerialPortName));

        return parts.Count > 0 ? string.Join(" + ", parts) : Localization.T("Tray.Source.None");
    }

    private static string Truncate(string s) => s.Length <= 127 ? s : s[..127];

    // ── Menü-Handler ─────────────────────────────────────────────────────────

    private async void OnSettingsClicked(object? sender, EventArgs e)
    {
        using var form = new SettingsForm(_settings);
        if (form.ShowDialog() == DialogResult.OK)
        {
            form.Result.Save();
            await ApplySettingsAsync(form.Result);
        }
    }

    private void OnSensorsClicked(object? sender, EventArgs e)
    {
        var sensors = _hardware.DiscoverSensors();
        using var picker = new SensorPickerForm(sensors, _settings.EnabledSensors, _hardware.GetValues);
        if (picker.ShowDialog() == DialogResult.OK)
        {
            _settings.EnabledSensors = picker.Result;
            _settings.Save();
        }
    }

    private void OnRestartAsAdminClicked(object? sender, EventArgs e)
    {
        var r = MessageBox.Show(
            Localization.T("Dialog.RestartAdmin.Body"),
            Localization.T("Dialog.RestartAdmin.Title"),
            MessageBoxButtons.OKCancel, MessageBoxIcon.Information);

        if (r != DialogResult.OK) return;

        var exePath = Environment.ProcessPath ?? Application.ExecutablePath;
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = exePath, Verb = "runas", UseShellExecute = true
            });
            Application.Exit();
        }
        catch (Exception ex)
        {
            MessageBox.Show(Localization.T("Dialog.RestartFailed.Body", ex.Message),
                Localization.T("Dialog.Error.Title"), MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private async void OnInstallPawnIoClicked(object? sender, EventArgs e)
    {
        if (PawnIoHelper.IsInstalled())
        {
            MessageBox.Show(Localization.T("Dialog.PawnIoAlready.Body"), "PulseMQTT",
                MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        await PawnIoHelper.OfferInstallAsync();
    }

    private async void OnCheckUpdateClicked(object? sender, EventArgs e)
        => await CheckForUpdateAsync(interactive: true);

    private async void OnExitClicked(object? sender, EventArgs e)
    {
        await ShutdownAsync();
        Application.Exit();
    }

    private async Task ShutdownAsync()
    {
        _pollTimer.Stop();
        _memoryTrimTimer.Stop();
        _updateTimer.Stop();
        _trayIcon.Visible = false;
        await _broker.StopAsync();
        await _serial.CloseAsync();
        _hardware.Dispose();
        _trayIcon.ContextMenuStrip?.Dispose();
        _trayIcon.Dispose();
        _pollTimer.Dispose();
        _memoryTrimTimer.Dispose();
        _updateTimer.Dispose();
    }

    // ── Selbst-Update ────────────────────────────────────────────────────────

    /// <summary>
    /// Prüft das neueste GitHub-Release und installiert es. Im interaktiven
    /// Modus (Menüklick) mit Dialogen und Rückfrage; im automatischen Modus
    /// still, nur mit einer Ballon-Meldung vor dem Neustart.
    /// </summary>
    private async Task CheckForUpdateAsync(bool interactive)
    {
        if (_updateInProgress) return;
        _updateInProgress = true;
        try
        {
            UpdateInfo? update;
            try
            {
                update = await UpdateService.CheckForUpdateAsync();
            }
            catch (Exception ex)
            {
                if (interactive)
                    MessageBox.Show(Localization.T("Update.CheckFailed.Body", ex.Message),
                        Localization.T("Update.Title"), MessageBoxButtons.OK, MessageBoxIcon.Error);
                return; // automatischer Check scheitert still (kein Internet o.ä.)
            }

            if (update is null)
            {
                if (interactive)
                    MessageBox.Show(
                        Localization.T("Update.None.Body", UpdateService.CurrentVersionDisplay),
                        Localization.T("Update.Title"), MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }

            if (interactive)
            {
                var r = MessageBox.Show(
                    Localization.T("Update.Available.Body",
                        update.TagName, UpdateService.CurrentVersionDisplay),
                    Localization.T("Update.Title"),
                    MessageBoxButtons.YesNo, MessageBoxIcon.Question);
                if (r != DialogResult.Yes) return;
            }
            else
            {
                _trayIcon.ShowBalloonTip(5000, "PulseMQTT",
                    Localization.T("Balloon.UpdateInstalling.Body", update.TagName),
                    ToolTipIcon.Info);
            }

            ProgressForm? progress = null;
            try
            {
                if (interactive)
                {
                    progress = new ProgressForm(
                        Localization.T("Update.Title"),
                        Localization.T("Update.Downloading.Body", update.TagName));
                    progress.Show();
                    Application.DoEvents();
                }

                var stagedExe = await UpdateService.DownloadAsync(update);
                UpdateService.ApplyAndRestart(stagedExe);
            }
            catch (Exception ex)
            {
                progress?.Close();
                if (interactive)
                    MessageBox.Show(Localization.T("Update.Failed.Body", ex.Message),
                        Localization.T("Update.Title"), MessageBoxButtons.OK, MessageBoxIcon.Error);
                else
                    _trayIcon.ShowBalloonTip(5000, "PulseMQTT",
                        Localization.T("Update.Failed.Body", ex.Message), ToolTipIcon.Warning);
                return;
            }
            finally
            {
                progress?.Close();
                progress?.Dispose();
            }

            // Neue EXE ist an Ort und Stelle, der neue Prozess startet bereits
            // und wartet auf den Single-Instance-Mutex → sauber beenden.
            await ShutdownAsync();
            Application.Exit();
        }
        finally
        {
            _updateInProgress = false;
        }
    }

    // ── Speicher-Trim ────────────────────────────────────────────────────────

    /// <summary>
    /// Kompaktiert den verwalteten Heap und gibt nicht mehr benötigte Seiten
    /// des Working Sets an das Betriebssystem zurück. Für eine Tray-App, die
    /// 99 % der Zeit idle ist, ist die kurze GC-Pause alle paar Minuten
    /// unkritisch – der Speicherverbrauch sinkt dafür deutlich.
    /// </summary>
    private void TrimMemory()
    {
        if (_isPublishing) return; // nicht mitten in einem Publish-Vorgang

        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
        GC.WaitForPendingFinalizers();

        try
        {
            using var process = Process.GetCurrentProcess();
            SetProcessWorkingSetSize(process.Handle, -1, -1);
        }
        catch { /* rein kosmetisch – ohne Trim läuft die App normal weiter */ }
    }

    [DllImport("kernel32.dll")]
    private static extern bool SetProcessWorkingSetSize(IntPtr process, nint minSize, nint maxSize);

    // ── Autostart ────────────────────────────────────────────────────────────

    private static void SetAutoStart(bool enabled)
    {
        const string taskName = "PulseMQTT";
        var exePath = Environment.ProcessPath ?? Application.ExecutablePath;

        if (enabled && IsAdmin)
        {
            RunSilent("schtasks.exe",
                $"/create /tn \"{taskName}\" /tr \"\\\"{exePath}\\\"\" /sc onlogon /rl highest /f");
            RemoveRegistryAutostart(taskName);
        }
        else if (enabled)
        {
            SetRegistryAutostart(taskName, exePath);
            RemoveScheduledTask(taskName);
        }
        else
        {
            SetRegistryAutostart(taskName, null);
            RemoveScheduledTask(taskName);
        }
    }

    private static void SetRegistryAutostart(string name, string? exePath)
    {
        using var key = Registry.CurrentUser.OpenSubKey(
            @"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
        if (key is null) return;
        if (exePath is not null) key.SetValue(name, $"\"{exePath}\"");
        else if (key.GetValue(name) is not null) key.DeleteValue(name, false);
    }

    private static void RemoveRegistryAutostart(string name) => SetRegistryAutostart(name, null);
    private static void RemoveScheduledTask(string name) =>
        RunSilent("schtasks.exe", $"/delete /tn \"{name}\" /f");

    private static void RunSilent(string exe, string args)
    {
        try
        {
            using var p = Process.Start(new ProcessStartInfo(exe, args)
                { UseShellExecute = false, CreateNoWindow = true });
            p?.WaitForExit(3000);
        }
        catch { }
    }
}
