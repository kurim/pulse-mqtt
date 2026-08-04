using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

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

    /// <summary>
    /// Klartext-Passwort zur Laufzeit. Wird NICHT direkt gespeichert –
    /// auf der Platte liegt nur die DPAPI-verschlüsselte Form
    /// (<see cref="MqttPasswordProtected"/>).
    /// </summary>
    [JsonIgnore]
    public string MqttPassword { get; set; } = "";

    /// <summary>
    /// DPAPI-verschlüsseltes Passwort (Base64, an das Windows-Benutzerkonto
    /// gebunden). Nur für die Persistenz gedacht.
    /// </summary>
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? MqttPasswordProtected { get; set; }

    /// <summary>
    /// Liest ältere settings.json ein, in denen das Passwort noch im
    /// Klartext unter "MqttPassword" stand. Wird beim nächsten Speichern
    /// automatisch in die verschlüsselte Form migriert.
    /// </summary>
    [JsonPropertyName("MqttPassword")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? LegacyMqttPassword { get; set; }

    /// <summary>
    /// Ob die Hardware-Daten über den eingebetteten MQTT-Broker gesendet werden.
    /// Unabhängig von <see cref="UseSerial"/> schaltbar – beide Wege können
    /// gleichzeitig aktiv sein.
    /// </summary>
    public bool UseMqtt { get; set; } = true;

    /// <summary>
    /// Ob die Hardware-Daten zusätzlich/alternativ als newline-getrennte JSON-
    /// Zeilen über einen seriellen Port (USB) gesendet werden, kompatibel zur
    /// Pulse ESP32-Firmware (115200 Baud, 8N1, kein Handshake).
    /// </summary>
    public bool UseSerial { get; set; } = false;

    /// <summary>Name des COM-Ports für die serielle Ausgabe, z. B. "COM3".</summary>
    public string SerialPortName { get; set; } = "";

    /// <summary>
    /// Wie das Gerät physisch angebunden ist: klassischer UART0-Adapter
    /// (CH340/CP210x) oder natives USB-CDC/"USB-Serial/JTAG" (z.B. ESP32-C3
    /// ohne separaten Bridge-Chip). Steuert DTR-Handling und Reset-Settle-
    /// Zeit beim Öffnen des Ports.
    /// </summary>
    public SerialConnectionMode SerialMode { get; set; } = SerialConnectionMode.UsbSerialJtag;

    /// <summary>
    /// Anzeigesprache: "auto" (Windows-Sprache), "de" oder "en".
    /// </summary>
    public string Language { get; set; } = "auto";

    /// <summary>
    /// Vom Benutzer ausgewählte Sensoren. Leer = noch nicht konfiguriert,
    /// beim nächsten Start werden Standardwerte vorausgewählt.
    /// </summary>
    public List<SensorEntry> EnabledSensors { get; set; } = [];

    // ── Persistenz ────────────────────────────────────────────────────────────

    private static readonly JsonSerializerOptions LoadOptions =
        new() { PropertyNameCaseInsensitive = true };

    private static readonly JsonSerializerOptions SaveOptions =
        new() { WriteIndented = true };

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
                var loaded = JsonSerializer.Deserialize<AppSettings>(json, LoadOptions);
                if (loaded is not null)
                {
                    loaded.RestorePassword();
                    return loaded;
                }
            }
        }
        catch { /* beschädigt → Standardwerte */ }
        return new AppSettings();
    }

    public void Save()
    {
        MqttPasswordProtected = ProtectPassword(MqttPassword);
        LegacyMqttPassword = null; // Klartext nie (wieder) auf die Platte schreiben

        Directory.CreateDirectory(ConfigDirectory);
        File.WriteAllText(ConfigPath, JsonSerializer.Serialize(this, SaveOptions));
    }

    public AppSettings Clone() => new()
    {
        MqttPort              = MqttPort,
        MqttTopic             = MqttTopic,
        UpdateIntervalSeconds = UpdateIntervalSeconds,
        StartWithWindows      = StartWithWindows,
        MqttUsername          = MqttUsername,
        MqttPassword          = MqttPassword,
        UseMqtt               = UseMqtt,
        UseSerial             = UseSerial,
        SerialPortName        = SerialPortName,
        SerialMode            = SerialMode,
        Language              = Language,
        EnabledSensors        = EnabledSensors
            .Select(e => new SensorEntry
                { Identifier = e.Identifier, MqttKey = e.MqttKey, IsEnabled = e.IsEnabled })
            .ToList()
    };

    // ── Passwort-Schutz (Windows DPAPI, an den angemeldeten Benutzer gebunden) ─

    private static readonly byte[] Entropy = Encoding.UTF8.GetBytes("PulseMQTT.MqttPassword.v1");

    private void RestorePassword()
    {
        if (!string.IsNullOrEmpty(MqttPasswordProtected))
        {
            MqttPassword = UnprotectPassword(MqttPasswordProtected);
        }
        else if (!string.IsNullOrEmpty(LegacyMqttPassword))
        {
            // Alte Klartext-Konfiguration – übernehmen, Migration erfolgt
            // beim nächsten Save().
            MqttPassword = LegacyMqttPassword;
        }
        LegacyMqttPassword = null;
    }

    private static string? ProtectPassword(string plain)
    {
        if (string.IsNullOrEmpty(plain)) return null;
        try
        {
            var protectedBytes = ProtectedData.Protect(
                Encoding.UTF8.GetBytes(plain), Entropy, DataProtectionScope.CurrentUser);
            return Convert.ToBase64String(protectedBytes);
        }
        catch
        {
            // DPAPI nicht verfügbar → Passwort lieber gar nicht speichern
            // als im Klartext.
            return null;
        }
    }

    private static string UnprotectPassword(string protectedBase64)
    {
        try
        {
            var plainBytes = ProtectedData.Unprotect(
                Convert.FromBase64String(protectedBase64), Entropy, DataProtectionScope.CurrentUser);
            return Encoding.UTF8.GetString(plainBytes);
        }
        catch
        {
            // Anderes Benutzerkonto / beschädigter Wert → wie "kein Passwort"
            return "";
        }
    }
}
