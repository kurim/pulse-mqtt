using System.Management;
using LibreHardwareMonitor.Hardware;

namespace PulseMQTT;

/// <summary>
/// Laufwerks-Sensoren über die Windows-Speicherverwaltung (WMI) statt über
/// LibreHardwareMonitor. LHM schickt bei jedem Poll SMART-/NVMe-Befehle direkt
/// an die Laufwerke und kann dabei USB-Massenspeicher (z. B. SD-Kartenleser)
/// stören. Hier werden nur interne Laufwerke (SATA/ATA/NVMe/SAS/SCSI/RAID)
/// abgefragt, USB-, SD- und virtuelle Laufwerke bleiben unberührt.
///
/// Quellen:
///   Temperatur  – MSFT_StorageReliabilityCounter (nur interne Laufwerke)
///   Auslastung  – Win32_PerfFormattedData_PerfDisk_PhysicalDisk (passive
///                 Windows-Zähler, kein Zugriff auf das Gerät)
/// Die Sensor-Identifier haben das Format "/winstorage/{DeviceId}/{typ}/0",
/// damit sie sich nicht mit LHM-Pfaden überschneiden.
/// </summary>
public sealed class WindowsStorageService
{
    private const string StorageScope = @"\\.\root\Microsoft\Windows\Storage";
    private const string CimScope     = @"\\.\root\cimv2";

    // MSFT_PhysicalDisk.BusType: 1 SCSI, 3 ATA, 8 RAID, 10 SAS, 11 SATA, 17 NVMe.
    // Alles andere (USB = 7, SD = 12, MMC = 13, virtuell, unbekannt) wird ignoriert.
    private static readonly HashSet<int> InternalBusTypes = [1, 3, 8, 10, 11, 17];

    // Temperatur ändert sich langsam; die Reliability-Abfrage geht über den
    // Storage-Provider und muss nicht bei jedem Poll laufen.
    private static readonly TimeSpan TemperatureCacheTime = TimeSpan.FromSeconds(15);

    private sealed record Disk(string DeviceId, string Name);

    private List<Disk> _disks = [];
    private readonly Dictionary<string, float> _temperatures = new(StringComparer.Ordinal);
    private DateTime _nextTemperatureReadUtc = DateTime.MinValue;

    private static string TemperatureId(string deviceId) => $"/winstorage/{deviceId}/temperature/0";
    private static string LoadId(string deviceId)        => $"/winstorage/{deviceId}/load/0";

    /// <summary>Ermittelt die internen Laufwerke neu und liefert ihre Sensoren.</summary>
    public List<AvailableSensor> Discover()
    {
        RefreshDisks();
        ReadTemperatures(force: true);
        var loads = ReadLoads();

        var result = new List<AvailableSensor>();
        foreach (var disk in _disks)
        {
            result.Add(new AvailableSensor
            {
                Identifier   = TemperatureId(disk.DeviceId),
                HardwareName = disk.Name,
                HardwareType = HardwareType.Storage,
                SensorName   = "Temperature",
                SensorType   = SensorType.Temperature,
                CurrentValue = _temperatures.TryGetValue(disk.DeviceId, out var t) ? t : null
            });
            result.Add(new AvailableSensor
            {
                Identifier   = LoadId(disk.DeviceId),
                HardwareName = disk.Name,
                HardwareType = HardwareType.Storage,
                SensorName   = "Total Activity",
                SensorType   = SensorType.Load,
                CurrentValue = loads.TryGetValue(disk.DeviceId, out var l) ? l : null
            });
        }
        return result;
    }

    /// <summary>Schreibt die aktuellen Werte in <paramref name="target"/>.</summary>
    public void CollectValues(Dictionary<string, float> target)
    {
        if (_disks.Count == 0)
            RefreshDisks();

        ReadTemperatures(force: false);
        foreach (var (deviceId, temp) in _temperatures)
            target[TemperatureId(deviceId)] = temp;

        foreach (var (deviceId, load) in ReadLoads())
            target[LoadId(deviceId)] = load;
    }

    private void RefreshDisks()
    {
        var disks = new List<Disk>();
        try
        {
            using var searcher = new ManagementObjectSearcher(
                StorageScope, "SELECT DeviceId, FriendlyName, BusType FROM MSFT_PhysicalDisk");
            foreach (ManagementObject disk in searcher.Get())
            {
                using (disk)
                {
                    var busType = Convert.ToInt32(disk["BusType"] ?? 0);
                    if (!InternalBusTypes.Contains(busType)) continue;

                    var id = Convert.ToString(disk["DeviceId"]) ?? "";
                    if (id.Length == 0) continue;
                    disks.Add(new Disk(id, Convert.ToString(disk["FriendlyName"]) ?? $"Disk {id}"));
                }
            }
        }
        catch
        {
            // WMI nicht verfügbar / keine Rechte → keine Laufwerks-Sensoren
        }
        _disks = disks;
    }

    private void ReadTemperatures(bool force)
    {
        if (!force && DateTime.UtcNow < _nextTemperatureReadUtc)
            return;
        _nextTemperatureReadUtc = DateTime.UtcNow + TemperatureCacheTime;

        _temperatures.Clear();
        foreach (var disk in _disks)
        {
            try
            {
                using var physical = new ManagementObjectSearcher(
                    StorageScope,
                    $"SELECT * FROM MSFT_PhysicalDisk WHERE DeviceId = '{disk.DeviceId}'");
                foreach (ManagementObject pd in physical.Get())
                {
                    using (pd)
                    {
                        // Wie Get-StorageReliabilityCounter: nur das Zähler-Objekt
                        // dieses einen (internen) Laufwerks wird abgefragt.
                        foreach (ManagementObject counter in pd.GetRelated(
                            "MSFT_StorageReliabilityCounter",
                            "MSFT_PhysicalDiskToStorageReliabilityCounter",
                            null, null, null, null, false, null))
                        {
                            using (counter)
                            {
                                if (counter["Temperature"] is null) continue;
                                var temp = Convert.ToSingle(counter["Temperature"]);
                                // 0 = vom Laufwerk nicht gemeldet
                                if (temp > 0) _temperatures[disk.DeviceId] = temp;
                            }
                        }
                    }
                }
            }
            catch
            {
                // Laufwerk meldet keine Zähler (oder keine Admin-Rechte) → überspringen
            }
        }
    }

    private Dictionary<string, float> ReadLoads()
    {
        var result = new Dictionary<string, float>(StringComparer.Ordinal);
        if (_disks.Count == 0) return result;

        try
        {
            using var searcher = new ManagementObjectSearcher(
                CimScope, "SELECT Name, PercentDiskTime FROM Win32_PerfFormattedData_PerfDisk_PhysicalDisk");
            foreach (ManagementObject perf in searcher.Get())
            {
                using (perf)
                {
                    // Name = "0 C: D:" (Laufwerksnummer + Laufwerksbuchstaben) oder "_Total"
                    var name = Convert.ToString(perf["Name"]) ?? "";
                    var number = name.Split(' ', 2)[0];
                    if (_disks.Any(d => d.DeviceId == number))
                        result[number] = Math.Min(100f, Convert.ToSingle(perf["PercentDiskTime"]));
                }
            }
        }
        catch
        {
            // Zähler nicht verfügbar → keine Auslastung
        }
        return result;
    }
}
