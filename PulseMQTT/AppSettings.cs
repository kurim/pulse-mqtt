using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace PulseMQTT;

public sealed class AppSettings
{
    public int MqttPort { get; set; } = 1883;
    public string MqttTopic { get; set; } = "pulsemqtt/hwinfo";
    public double UpdateIntervalSeconds { get; set; } = 2.0;
    public bool StartWithWindows { get; set; }

    /// <summary>
    /// Optionale Zugangsdaten für den eingebetteten Broker. Leer = keine
    /// Authentifizierung, jeder Client darf sich verbinden.
    /// </summary>
    public string MqttUsername { get; set; } = "";
    public string MqttPassword { get; set; } = "";

    /// <summary>
    /// Vom Benutzer ausgewählte Sensoren. Leer = noch nicht konfiguriert,
    /// beim nächsten Start werden Standardwerte vorausgewählt.
    /// </summary>
    public List<SensorEntry> EnabledSensors { get; set; } = [];

    // ── Persistenz ────────────────────────────────────────────────────────────

    private static string ConfigDirectory =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "PulseMQTT");

    private static string ConfigPath => Path.Combine(ConfigDirectory, "settings.json");

    public static AppSettings Load()
    {
        try
        {
            if (File.Exists(ConfigPath))
            {
                var json = File.ReadAllText(ConfigPath);
                var loaded = JsonSerializer.Deserialize<AppSettings>(json,
                    new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
                if (loaded is not null) return loaded;
            }
        }
        catch { /* beschädigt → Standardwerte */ }
        return new AppSettings();
    }

    public void Save()
    {
        Directory.CreateDirectory(ConfigDirectory);
        File.WriteAllText(ConfigPath,
            JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
    }

    public AppSettings Clone()
    {
        var json = JsonSerializer.Serialize(this);
        return JsonSerializer.Deserialize<AppSettings>(json)!;
    }
}
