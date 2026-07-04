using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Linq;
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
    private readonly MqttBrokerService _broker = new();
    private readonly HardwareMonitorService _hardware = new();
    private AppSettings _settings;
    private bool _accessWarningShown;
    private bool _isPublishing;

    private static readonly bool IsAdmin =
        new WindowsPrincipal(WindowsIdentity.GetCurrent())
            .IsInRole(WindowsBuiltInRole.Administrator);

    public TrayAppContext()
    {
        _settings = AppSettings.Load();

        var menu = new ContextMenuStrip();
        menu.Items.Add("Einstellungen...",        null, OnSettingsClicked);
        menu.Items.Add("Sensoren auswählen...",   null, OnSensorsClicked);
        menu.Items.Add(new ToolStripSeparator());

        if (!IsAdmin)
            menu.Items.Add("🔒 Als Administrator neu starten", null, OnRestartAsAdminClicked);
        else
        {
            var adminItem = new ToolStripMenuItem("✔ Läuft als Administrator") { Enabled = false };
            menu.Items.Add(adminItem);
        }

        if (PawnIoHelper.IsInstalled())
        {
            menu.Items.Add(new ToolStripMenuItem("✔ PawnIO installiert") { Enabled = false });
        }
        else
        {
            menu.Items.Add("PawnIO installieren...", null, OnInstallPawnIoClicked);
        }
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Beenden", null, OnExitClicked);

        _trayIcon = new NotifyIcon
        {
            Icon             = LoadAppIcon(),
            Text             = "PulseMQTT – wird gestartet...",
            ContextMenuStrip = menu,
            Visible          = true
        };
        _trayIcon.DoubleClick += OnSensorsClicked;

        _pollTimer = new System.Windows.Forms.Timer();
        _pollTimer.Tick += async (_, _) => await PublishSnapshotAsync();

        _ = StartupAsync();
    }

    private async Task StartupAsync()
    {
        await ApplySettingsAsync(_settings);

        // Sensor-Erstkonfiguration wenn noch nichts ausgewählt wurde
        if (_settings.EnabledSensors.Count == 0)
            await AutoConfigureSensorsAsync();

        if (!PawnIoHelper.IsInstalled())
            await PawnIoHelper.OfferInstallAsync();
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

    private static Icon LoadAppIcon()
    {
        using var stream = typeof(TrayAppContext).Assembly
            .GetManifestResourceStream("PulseMQTT.app.ico");
        if (stream is not null) return new Icon(stream);
        return SystemIcons.Application;
    }

    private async Task ApplySettingsAsync(AppSettings settings)
    {
        _settings = settings;
        _pollTimer.Stop();
        _pollTimer.Interval = Math.Max(250, (int)(settings.UpdateIntervalSeconds * 1000));

        try
        {
            await _broker.StartAsync(settings.MqttPort);
            _trayIcon.Text = Truncate($"PulseMQTT – Port {settings.MqttPort} | {settings.MqttTopic}");
        }
        catch (Exception ex)
        {
            _trayIcon.Text = Truncate($"PulseMQTT – MQTT-Fehler: {ex.Message}");
            _trayIcon.ShowBalloonTip(5000, "PulseMQTT",
                $"MQTT-Broker konnte nicht auf Port {settings.MqttPort} gestartet werden:\n{ex.Message}",
                ToolTipIcon.Error);
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
            var values  = _hardware.GetValues();
            var enabled = _settings.EnabledSensors.Where(e => e.IsEnabled).ToList();

            if (!_accessWarningShown && enabled.Count > 0)
            {
                var hasData = enabled.Any(e =>
                    values.TryGetValue(e.Identifier, out var v) && v > 0);

                if (!hasData && !IsAdmin && PawnIoHelper.IsInstalled())
                {
                    _accessWarningShown = true;
                    _trayIcon.ShowBalloonTip(10_000, "PulseMQTT – Eingeschränkter Zugriff",
                        "Sensor-Werte nicht verfügbar (PawnIO erlaubt Zugriff nur für Admins).\n" +
                        "Rechtsklick → Als Administrator neu starten.",
                        ToolTipIcon.Warning);
                }
            }

            // Payload dynamisch aus gewählten Sensoren aufbauen
            var payload = new Dictionary<string, float>();
            foreach (var entry in enabled)
                if (values.TryGetValue(entry.Identifier, out var val))
                    payload[entry.MqttKey] = MathF.Round(val, 1);

            await _broker.PublishAsync(_settings.MqttTopic,
                JsonSerializer.Serialize(payload));

            // Tooltip: die ersten 3-4 Werte anzeigen
            var preview = string.Join("  │  ", payload
                .Take(4)
                .Select(kv => $"{kv.Key}: {kv.Value}"));

            _trayIcon.Text = Truncate(
                $"PulseMQTT{(IsAdmin ? " 🔒" : "")} – {_broker.ConnectedClients} Client(s) – Port {_settings.MqttPort}\n" +
                preview);
        }
        catch (Exception ex)
        {
            _trayIcon.Text = Truncate($"PulseMQTT – Fehler: {ex.Message}");
        }
        finally
        {
            _isPublishing = false;
        }
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
            "PulseMQTT wird als Administrator neu gestartet.\n" +
            "Windows fragt einmalig nach dem Admin-Passwort (UAC).",
            "Als Administrator neu starten",
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
            MessageBox.Show($"Neustart fehlgeschlagen:\n{ex.Message}",
                "Fehler", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private async void OnInstallPawnIoClicked(object? sender, EventArgs e)
    {
        if (PawnIoHelper.IsInstalled())
        {
            MessageBox.Show("PawnIO ist bereits installiert.", "PulseMQTT",
                MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        await PawnIoHelper.OfferInstallAsync();
    }

    private async void OnExitClicked(object? sender, EventArgs e)
    {
        _pollTimer.Stop();
        _trayIcon.Visible = false;
        await _broker.StopAsync();
        _hardware.Dispose();
        Application.Exit();
    }

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
