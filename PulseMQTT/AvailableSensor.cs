using LibreHardwareMonitor.Hardware;

namespace PulseMQTT;

/// <summary>
/// Ein zur Laufzeit von LibreHardwareMonitor entdeckter Sensor.
/// Wird beim Start und auf Anfrage neu ermittelt.
/// </summary>
public sealed class AvailableSensor
{
    /// <summary>Eindeutiger LHM-Pfad, z. B. "/intelcpu/0/temperature/0"</summary>
    public string Identifier { get; init; } = "";

    public string HardwareName { get; init; } = "";
    public HardwareType HardwareType { get; init; }
    public string SensorName { get; init; } = "";
    public SensorType SensorType { get; init; }
    public float? CurrentValue { get; set; }

    public string Unit => SensorType switch
    {
        SensorType.Temperature => "°C",
        SensorType.Load        => "%",
        SensorType.Power       => "W",
        SensorType.Fan         => "RPM",
        SensorType.Voltage     => "V",
        SensorType.Clock       => "MHz",
        SensorType.Data        => "GB",
        SensorType.SmallData   => "MB",
        _                      => ""
    };

    public string TypeLabel => SensorType switch
    {
        SensorType.Temperature => "Temperatur",
        SensorType.Load        => "Auslastung",
        SensorType.Power       => "Leistung",
        SensorType.Fan         => "Lüfter",
        SensorType.Voltage     => "Spannung",
        SensorType.Clock       => "Takt",
        SensorType.Data        => "Daten",
        _                      => SensorType.ToString()
    };

    public string HardwareCategory => HardwareType switch
    {
        HardwareType.Cpu         => "CPU",
        HardwareType.GpuNvidia   => "GPU (NVIDIA)",
        HardwareType.GpuAmd      => "GPU (AMD)",
        HardwareType.GpuIntel    => "GPU (Intel)",
        HardwareType.Memory      => "RAM",
        HardwareType.Motherboard => "Mainboard",
        HardwareType.Storage     => "Speicher",
        HardwareType.Network     => "Netzwerk",
        _                        => HardwareType.ToString()
    };
}
