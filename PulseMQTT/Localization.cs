using System;
using System.Collections.Generic;
using System.Globalization;

namespace PulseMQTT;

/// <summary>
/// Sehr einfache Übersetzungsschicht (Deutsch/Englisch). "auto" erkennt die
/// Sprache anhand der Windows-UI-Kultur; ansonsten wird die im Dropdown
/// gewählte Sprache erzwungen.
/// </summary>
public static class Localization
{
    private static string _languageCode = "auto";

    /// <summary>"auto", "de" oder "en".</summary>
    public static string LanguageCode
    {
        get => _languageCode;
        set => _languageCode = value is "de" or "en" ? value : "auto";
    }

    private static bool IsGerman =>
        LanguageCode switch
        {
            "de" => true,
            "en" => false,
            _ => CultureInfo.CurrentUICulture.TwoLetterISOLanguageName
                .Equals("de", StringComparison.OrdinalIgnoreCase)
        };

    /// <summary>Übersetzt den Schlüssel, optional mit string.Format-Argumenten.</summary>
    public static string T(string key, params object[] args)
    {
        if (!Strings.TryGetValue(key, out var pair))
            return key;

        var text = IsGerman ? pair.De : pair.En;
        return args.Length == 0 ? text : string.Format(text, args);
    }

    private static readonly Dictionary<string, (string De, string En)> Strings = new()
    {
        ["App.AlreadyRunning.Body"] = (
            "PulseMQTT läuft bereits im Hintergrund.\nSchau im Systray nach dem Symbol.",
            "PulseMQTT is already running in the background.\nCheck the system tray for the icon."),

        // ── Tray-Menü ───────────────────────────────────────────────────────
        ["Menu.Settings"] = ("Einstellungen...", "Settings..."),
        ["Menu.SelectSensors"] = ("Sensoren auswählen...", "Select sensors..."),
        ["Menu.RestartAsAdmin"] = ("🔒 Als Administrator neu starten", "🔒 Restart as administrator"),
        ["Menu.RunningAsAdmin"] = ("✔ Läuft als Administrator", "✔ Running as administrator"),
        ["Menu.PawnIoInstalled"] = ("✔ PawnIO installiert", "✔ PawnIO installed"),
        ["Menu.InstallPawnIo"] = ("PawnIO installieren...", "Install PawnIO..."),
        ["Menu.Exit"] = ("Beenden", "Exit"),

        // ── Tray-Text ───────────────────────────────────────────────────────
        ["Tray.Starting"] = ("{0} – wird gestartet...", "{0} – starting..."),
        ["Tray.PortTopic"] = ("{0} – Port {1} | {2}", "{0} – port {1} | {2}"),
        ["Tray.MqttError"] = ("{0} – MQTT-Fehler: {1}", "{0} – MQTT error: {1}"),
        ["Tray.Error"] = ("{0} – Fehler: {1}", "{0} – Error: {1}"),
        ["Tray.Status"] = ("{0}{1} – {2} Client(s) – Port {3}\n{4}", "{0}{1} – {2} client(s) – port {3}\n{4}"),

        ["Balloon.MqttStartFailed.Body"] = (
            "MQTT-Broker konnte nicht auf Port {0} gestartet werden:\n{1}",
            "MQTT broker could not be started on port {0}:\n{1}"),
        ["Balloon.RestrictedAccess.Title"] = (
            "PulseMQTT – Eingeschränkter Zugriff", "PulseMQTT – Restricted access"),
        ["Balloon.RestrictedAccess.Body"] = (
            "Sensor-Werte nicht verfügbar (PawnIO erlaubt Zugriff nur für Admins).\n" +
            "Rechtsklick → Als Administrator neu starten.",
            "Sensor values unavailable (PawnIO only allows access for admins).\n" +
            "Right-click → Restart as administrator."),

        ["Dialog.RestartAdmin.Title"] = ("Als Administrator neu starten", "Restart as administrator"),
        ["Dialog.RestartAdmin.Body"] = (
            "PulseMQTT wird als Administrator neu gestartet.\n" +
            "Windows fragt einmalig nach dem Admin-Passwort (UAC).",
            "PulseMQTT will restart as administrator.\n" +
            "Windows will ask once for admin approval (UAC)."),
        ["Dialog.Error.Title"] = ("Fehler", "Error"),
        ["Dialog.RestartFailed.Body"] = ("Neustart fehlgeschlagen:\n{0}", "Restart failed:\n{0}"),
        ["Dialog.PawnIoAlready.Body"] = ("PawnIO ist bereits installiert.", "PawnIO is already installed."),

        // ── Einstellungen-Dialog ────────────────────────────────────────────
        ["Settings.Title"] = ("PulseMQTT – Einstellungen", "PulseMQTT – Settings"),
        ["Settings.Port"] = ("MQTT-Port:", "MQTT port:"),
        ["Settings.Topic"] = ("MQTT-Topic:", "MQTT topic:"),
        ["Settings.Username"] = ("MQTT-Benutzername:", "MQTT username:"),
        ["Settings.Password"] = ("MQTT-Passwort:", "MQTT password:"),
        ["Settings.Interval"] = ("Update-Intervall (Sek.):", "Update interval (sec.):"),
        ["Settings.AutoStart"] = ("Mit Windows starten:", "Start with Windows:"),
        ["Settings.Language"] = ("Sprache:", "Language:"),
        ["Settings.Hint"] = (
            "Topic muss mit der Firmware-Konfiguration übereinstimmen.",
            "Topic must match the firmware configuration."),
        ["Settings.Language.Auto"] = ("Automatisch", "Automatic"),
        ["Settings.Language.German"] = ("Deutsch", "Deutsch"),
        ["Settings.Language.English"] = ("English", "English"),

        // ── Sensoren-Dialog ─────────────────────────────────────────────────
        ["Sensors.Title"] = ("PulseMQTT – Sensoren konfigurieren", "PulseMQTT – Configure sensors"),
        ["Sensors.Hint"] = (
            "Wähle die Sensoren, die per MQTT publiziert werden. " +
            "Die MQTT-Schlüssel müssen mit der Firmware-Konfiguration übereinstimmen.\n" +
            "⚑ Vorausgewählte Standardwerte sind mit der Pulse ESP32-Firmware kompatibel.",
            "Choose the sensors that are published via MQTT. " +
            "The MQTT keys must match the firmware configuration.\n" +
            "⚑ Pre-selected defaults are compatible with the Pulse ESP32 firmware."),
        ["Sensors.Col.Hardware"] = ("Hardware", "Hardware"),
        ["Sensors.Col.Type"] = ("Typ", "Type"),
        ["Sensors.Col.Sensor"] = ("Sensor", "Sensor"),
        ["Sensors.Col.MqttKey"] = ("MQTT-Schlüssel", "MQTT key"),
        ["Sensors.Col.Value"] = ("Aktuell", "Current"),
        ["Sensors.MissingKey.Title"] = ("Fehlender MQTT-Schlüssel", "Missing MQTT key"),
        ["Sensors.MissingKey.Body"] = (
            "Sensor \"{0}\" ist aktiviert, hat aber keinen MQTT-Schlüssel.\n" +
            "Bitte einen Schlüssel eingeben oder den Sensor deaktivieren.",
            "Sensor \"{0}\" is enabled but has no MQTT key.\n" +
            "Please enter a key or disable the sensor."),
        ["Sensors.DupeKeys.Title"] = ("Doppelte Schlüssel", "Duplicate keys"),
        ["Sensors.DupeKeys.Body"] = (
            "Folgende MQTT-Schlüssel sind mehrfach vergeben: {0}\n" +
            "Bitte jeden Schlüssel nur einmal verwenden.",
            "The following MQTT keys are used more than once: {0}\n" +
            "Please use each key only once."),

        // ── Buttons ─────────────────────────────────────────────────────────
        ["Button.Save"] = ("Speichern", "Save"),
        ["Button.Cancel"] = ("Abbrechen", "Cancel"),
        ["Button.Defaults"] = ("Standardwerte", "Defaults"),

        // ── PawnIO ──────────────────────────────────────────────────────────
        ["PawnIo.NotFound.Title"] = ("PawnIO nicht gefunden – PulseMQTT", "PawnIO not found – PulseMQTT"),
        ["PawnIo.NotFound.Body"] = (
            "Der PawnIO-Treiber ist nicht installiert.\n\n" +
            "PawnIO stellt den Low-Level-Hardwarezugriff für LibreHardwareMonitor bereit " +
            "(Nachfolger von WinRing0). Ohne ihn sind keine CPU-/GPU-Temperatur- und " +
            "Leistungswerte verfügbar – nur Last-Prozentwerte.\n\n" +
            "Soll PulseMQTT den PawnIO-Installer jetzt herunterladen und starten?\n" +
            "Windows fragt anschließend nach Administrator-Rechten " +
            "(der Installer benötigt sie für die Treiber-Installation).\n\n" +
            "Alternativ: Manuell von https://pawnio.eu/ herunterladen.",
            "The PawnIO driver is not installed.\n\n" +
            "PawnIO provides low-level hardware access for LibreHardwareMonitor " +
            "(successor to WinRing0). Without it, no CPU/GPU temperature or power " +
            "values are available – only load percentages.\n\n" +
            "Should PulseMQTT download and start the PawnIO installer now?\n" +
            "Windows will then ask for administrator rights " +
            "(the installer needs them to install the driver).\n\n" +
            "Alternatively: download manually from https://pawnio.eu/."),
        ["PawnIo.Downloading.Title"] = ("PawnIO herunterladen…", "Downloading PawnIO…"),
        ["PawnIo.Downloading.Body"] = ("Lade Installer herunter...", "Downloading installer..."),
        ["PawnIo.DownloadFailed.Title"] = ("Download-Fehler", "Download error"),
        ["PawnIo.DownloadFailed.Body"] = (
            "Download fehlgeschlagen:\n{0}\n\n" +
            "Bitte den Installer manuell von https://pawnio.eu/ herunterladen.",
            "Download failed:\n{0}\n\n" +
            "Please download the installer manually from https://pawnio.eu/."),
        ["PawnIo.Installing.Title"] = ("PawnIO wird installiert", "Installing PawnIO"),
        ["PawnIo.Installing.Body"] = (
            "Der PawnIO-Installer wurde gestartet.\n\n" +
            "Nach der Installation bitte PulseMQTT neu starten, " +
            "damit der Treiber erkannt wird.",
            "The PawnIO installer has been started.\n\n" +
            "Please restart PulseMQTT after installation " +
            "so the driver is detected."),
        ["PawnIo.StartFailed.Body"] = (
            "Installer konnte nicht gestartet werden:\n{0}",
            "Installer could not be started:\n{0}"),
    };
}
