namespace PulseMQTT;

/// <summary>
/// Speichert die Benutzer-Konfiguration für einen einzelnen Sensor:
/// ob er publiziert wird und unter welchem MQTT-Schlüssel.
/// </summary>
public sealed class SensorEntry
{
    /// <summary>LHM-Identifier, z. B. "/intelcpu/0/temperature/0"</summary>
    public string Identifier { get; set; } = "";

    /// <summary>
    /// MQTT-Feldname im JSON-Payload, z. B. "cpu_temp".
    /// Muss eindeutig sein; Firmware erwartet konkrete Namen.
    /// </summary>
    public string MqttKey { get; set; } = "";

    public bool IsEnabled { get; set; } = true;
}
