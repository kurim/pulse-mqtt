using System;
using System.Collections.Generic;
using System.Linq;
using LibreHardwareMonitor.Hardware;

namespace PulseMQTT;

/// <summary>
/// Kapselt LibreHardwareMonitorLib.
///
/// Zwei öffentliche Operationen:
///   DiscoverSensors() – alle gefundenen Sensoren mit aktuellem Wert
///   GetValues()       – Dictionary [Identifier → aktueller Wert]
///
/// Temperaturen und Leistung erfordern Admin-Rechte (PawnIO-Zugriff).
/// </summary>
public sealed class HardwareMonitorService : IDisposable
{
    private readonly Computer _computer;
    private readonly UpdateVisitor _visitor = new();

    // Hardware-Typen die standardmäßig überwacht werden
    public HardwareMonitorService()
    {
        _computer = new Computer
        {
            IsCpuEnabled         = true,
            IsGpuEnabled         = true,
            IsMemoryEnabled      = true,
            IsMotherboardEnabled = false,
            IsStorageEnabled     = false,
            IsNetworkEnabled     = false,
            IsControllerEnabled  = false,
            IsBatteryEnabled     = false,
            IsPsuEnabled         = false
        };
        _computer.Open();
    }

    /// <summary>
    /// Liest alle Sensor-Werte einmalig aus und gibt eine Kopie als Liste zurück.
    /// Wird bei jedem Poll-Intervall und beim Öffnen des Sensor-Pickers aufgerufen.
    /// </summary>
    public List<AvailableSensor> DiscoverSensors()
    {
        _computer.Accept(_visitor);
        var result = new List<AvailableSensor>();

        foreach (var hw in _computer.Hardware)
            CollectFrom(hw, result);

        return result;
    }

    private static void CollectFrom(IHardware hw, List<AvailableSensor> target)
    {
        foreach (var sensor in hw.Sensors)
        {
            // Nur sinnvolle Typen
            if (sensor.SensorType is not (
                SensorType.Temperature or SensorType.Load or
                SensorType.Power or SensorType.Fan or
                SensorType.Voltage or SensorType.Clock or
                SensorType.Data or SensorType.SmallData))
                continue;

            target.Add(new AvailableSensor
            {
                Identifier   = sensor.Identifier.ToString(),
                HardwareName = hw.Name,
                HardwareType = hw.HardwareType,
                SensorName   = sensor.Name,
                SensorType   = sensor.SensorType,
                CurrentValue = sensor.Value
            });
        }

        // Rekursiv in SubHardware (z. B. CPU-Kerne unter Motherboard)
        foreach (var sub in hw.SubHardware)
            CollectFrom(sub, target);
    }

    /// <summary>
    /// Schneller Pfad für den Poll-Timer: nur Werte lesen, keine neue Liste aufbauen.
    /// </summary>
    public Dictionary<string, float> GetValues()
    {
        _computer.Accept(_visitor);
        var dict = new Dictionary<string, float>(StringComparer.Ordinal);

        foreach (var hw in _computer.Hardware)
            CollectValues(hw, dict);

        return dict;
    }

    private static void CollectValues(IHardware hw, Dictionary<string, float> dict)
    {
        foreach (var sensor in hw.Sensors)
            if (sensor.Value.HasValue)
                dict[sensor.Identifier.ToString()] = sensor.Value.Value;

        foreach (var sub in hw.SubHardware)
            CollectValues(sub, dict);
    }

    public void Dispose() => _computer.Close();

    // ── Standard-Sensor-Auswahl ──────────────────────────────────────────────

    /// <summary>
    /// Wählt aus einer Liste von entdeckten Sensoren sinnvolle Standardwerte
    /// mit vorbelegten MQTT-Schlüsseln aus (kompatibel mit CYD-Firmware).
    /// </summary>
    public static List<SensorEntry> BuildDefaultSelection(List<AvailableSensor> sensors)
    {
        var result = new List<SensorEntry>();

        // CPU
        TryAdd(result, sensors, HardwareType.Cpu, SensorType.Load,
            ["CPU Total", "CPU Package", "CPU"],
            "cpu_load");

        TryAdd(result, sensors, HardwareType.Cpu, SensorType.Temperature,
            ["CPU Package", "Core Average", "Tctl/Tdie", "Tdie", "Package", "CPU"],
            "cpu_temp");

        TryAdd(result, sensors, HardwareType.Cpu, SensorType.Power,
            ["CPU Package", "Package", "CPU Core Power (SVI2 TFN)", "CPU Package Power", "CPU"],
            "cpu_power");

        // GPU (NVIDIA, AMD, Intel)
        foreach (var gpuType in new[] { HardwareType.GpuNvidia, HardwareType.GpuAmd, HardwareType.GpuIntel })
        {
            TryAdd(result, sensors, gpuType, SensorType.Load,
                ["GPU Core", "D3D 3D", "GPU"],
                "gpu_load");

            TryAdd(result, sensors, gpuType, SensorType.Temperature,
                ["GPU Core", "GPU Hot Spot", "GPU"],
                "gpu_temp");

            TryAdd(result, sensors, gpuType, SensorType.Power,
                ["GPU Package", "GPU Power", "Total Board Power", "GPU SoC", "GPU"],
                "gpu_power");
        }

        return result;
    }

    private static void TryAdd(
        List<SensorEntry> result,
        List<AvailableSensor> sensors,
        HardwareType hwType,
        SensorType sensorType,
        string[] namePrefs,
        string mqttKey)
    {
        // Nicht doppelt hinzufügen (z. B. wenn mehrere GPU-Typen versucht werden)
        if (result.Any(e => e.MqttKey == mqttKey)) return;

        AvailableSensor? found = null;
        foreach (var name in namePrefs)
        {
            found = sensors.FirstOrDefault(s =>
                s.HardwareType == hwType &&
                s.SensorType   == sensorType &&
                s.SensorName.Contains(name, StringComparison.OrdinalIgnoreCase));
            if (found is not null) break;
        }

        if (found is null) return;

        result.Add(new SensorEntry
        {
            Identifier = found.Identifier,
            MqttKey    = mqttKey,
            IsEnabled  = true
        });
    }

    private sealed class UpdateVisitor : IVisitor
    {
        public void VisitComputer(IComputer c) => c.Traverse(this);
        public void VisitHardware(IHardware hw)
        {
            hw.Update();
            foreach (var sub in hw.SubHardware) sub.Accept(this);
        }
        public void VisitSensor(ISensor s) { }
        public void VisitParameter(IParameter p) { }
    }
}
