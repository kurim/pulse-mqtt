using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace PCBridge;

public sealed class AppSettings
{
    public int MqttPort { get; set; } = 1883;
    public string MqttTopic { get; set; } = "pcbridge/hwinfo";
    public double UpdateIntervalSeconds { get; set; } = 2.0;
    public bool StartWithWindows { get; set; }

    /// <summary>
    /// Vom Benutzer ausgewählte Sensoren. Leer = noch nicht konfiguriert,
    /// beim nächsten Start werden Standardwerte vorausgewählt.
    /// </summary>
    public List<SensorEntry> EnabledSensors { get; set; } = [];

    // ── Persistenz ────────────────────────────────────────────────────────────

    private static string ConfigDirectory =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "PCBridge");

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
