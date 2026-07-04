# PulseMQTT

**PulseMQTT** is a lightweight Windows tray application that publishes PC hardware data (CPU/GPU load, temperatures, power consumption) via MQTT. It is primarily aimed at small displays like the **CYD (Cheap Yellow Display / ESP32-2432S028)** but works with any MQTT-capable client.

> 🇩🇪 [Deutsche Version](README.de.md)

---

## Features

- **Embedded MQTT broker** – no separate Mosquitto server required; the CYD board (or any other client) connects directly to PulseMQTT
- **Flexible sensor selection** – all sensors detected by LibreHardwareMonitor are listed; you choose which ones to publish and under which MQTT key
- **Configurable MQTT keys** – defaults are compatible with the Pulse ESP32 firmware (`cpu_load`, `cpu_temp`, `cpu_power`, `gpu_load`, `gpu_temp`, `gpu_power`)
- **Tray icon** – runs silently in the background, live values shown in the tooltip
- **No admin rights required to start** – optional "Restart as Administrator" entry in the menu for full sensor access (temperatures/watts require PawnIO driver access)
- **Autostart** – via registry (standard user) or Task Scheduler (admin mode, no UAC prompt)
- **PawnIO assistant** – detects whether the driver is installed and offers download + installation
- **Single-file EXE** – optionally publishable as a portable single file (no .NET setup required)

---

## Requirements

| Component | Version | Notes |
|---|---|---|
| Windows | 10 / 11 (x64) | |
| .NET 8 Desktop Runtime | 8.0 or later | Only required for framework-dependent build |
| PawnIO driver | current | For temperature and power sensors, one-time admin install |

### PawnIO

PulseMQTT uses [LibreHardwareMonitorLib](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor), which internally relies on the **PawnIO driver** (successor to WinRing0) for low-level hardware access.

- **Download:** https://pawnio.eu/
- Installation requires admin rights once (kernel driver)
- PulseMQTT will automatically offer installation on first launch
- Without PawnIO, only CPU load values are available (no temperatures, no watts)
- For full sensor data: launch PulseMQTT elevated via **right-click → Restart as Administrator**

---

## Installation & Launch

### Option A – Quick start (framework-dependent, ~8 MB)

1. Install the [.NET 8 Desktop Runtime](https://dotnet.microsoft.com/download/dotnet/8.0) (one-time)
2. Run `PulseMQTT.exe`
3. On first launch: confirm or adjust the sensor selection

### Option B – Portable single-file EXE (~70 MB, no .NET required)

```powershell
.\publish.ps1 -Mode NoTrim
```

The EXE will be located at `publish_notrim\PulseMQTT.exe` and runs on any Windows x64 machine without additional software.

### Option C – Visual Studio Publish

Right-click project → **Publish** → select profile:

| Profile | Size | Requirement |
|---|---|---|
| `SingleFile` | ~20 MB | none (self-contained, trimmed) |
| `SingleFile_NoTrim` | ~70 MB | none (self-contained, safer) |
| `FrameworkDependent` | ~8 MB | .NET 8 Runtime installed |

---

## Usage

### First Launch

On the very first start:
1. PulseMQTT automatically discovers all available sensors
2. Sensible defaults are pre-selected (Pulse ESP32 firmware-compatible MQTT keys)
3. The **sensor picker** opens for confirmation or customization

### Tray Menu

| Entry | Function |
|---|---|
| **Settings…** | Port, topic, update interval, autostart |
| **Select sensors…** | Open sensor picker (also double-click the icon) |
| 🔒 **Restart as Administrator** | For full temperature/watt values (only shown without admin) |
| ✔ **Running as Administrator** | Indicates elevated mode (grayed out) |
| ✔ **PawnIO installed** | PawnIO detected (grayed out) |
| **Install PawnIO…** | Download + install the driver (only shown if missing) |
| **Exit** | Quit the app |

### Sensor Picker

Opened via **right-click → Select sensors…** or **double-click** on the tray icon.

- All detected sensors are displayed in a table (hardware, type, name, current value)
- **Checkbox** activates the sensor
- **MQTT key** (yellow column) is freely editable – this name appears in the JSON payload
- **Default values** restores the pre-selection compatible with the Pulse ESP32 firmware
- Live values update every 2 seconds while the dialog is open

### Settings

| Field | Default | Description |
|---|---|---|
| MQTT port | `1883` | Port of the embedded broker |
| MQTT topic | `pulsemqtt/hwinfo` | Topic under which the payload is published |
| MQTT username | empty | Optional – if set, clients must authenticate |
| MQTT password | empty | Optional – used together with the username |
| Update interval | `2.0 s` | How often new values are sent |
| Start with Windows | off | Autostart (registry or Task Scheduler) |
| Language | Automatic | Automatic (follows Windows), German, or English |

If username and password are left empty, the broker accepts anonymous connections (default). Set both to require clients to authenticate before they can connect.

Saved to `%AppData%\PulseMQTT\settings.json`.

---

## MQTT Payload

PulseMQTT publishes a JSON object with the user-configured fields.
Default selection (Pulse ESP32 firmware-compatible):

```json
{
  "cpu_load":  42.5,
  "cpu_temp":  58.0,
  "cpu_power": 65.3,
  "gpu_load":  12.0,
  "gpu_temp":  41.0,
  "gpu_power": 28.5
}
```

The field names correspond to the MQTT keys you assign in the sensor picker. Custom keys are possible but must also be updated in the firmware/display client accordingly.

### Connecting the CYD Board

In the CYD web interface (captive portal or `http://<CYD-IP>`):

| Field | Value |
|---|---|
| `mqtt_host` | LAN IP of the PC running PulseMQTT |
| `mqtt_port` | `1883` (or as configured) |
| `mqtt_topic` | `pulsemqtt/hwinfo` (or as configured) |

---

## Autostart

| Mode | Method | UAC on start |
|---|---|---|
| Standard user | `HKCU\...\Run` registry entry | none |
| Administrator | Windows Task Scheduler (`/rl highest`) | none |

When PulseMQTT is running as admin and "Start with Windows" is enabled, a Task Scheduler task is created that launches elevated at login without a UAC prompt.

---

## Building from Source

**Requirements:**
- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)
- Windows (WinForms target)

```powershell
git clone https://github.com/<your-username>/PulseMQTT.git
cd PulseMQTT
dotnet restore
dotnet run --project PulseMQTT
```

**Create single-file EXE:**

```powershell
cd PulseMQTT
.\publish.ps1                    # NoTrim ~70 MB (recommended)
.\publish.ps1 -Mode Fx           # Framework-dependent ~8 MB
```

---

## Project Structure

```
PulseMQTT.sln
PulseMQTT/
├── Program.cs                  # Entry point, single-instance check
├── TrayAppContext.cs           # Tray icon, menu, poll timer, orchestration
├── AppSettings.cs              # Settings (JSON under %AppData%)
├── SensorEntry.cs              # Saved sensor configuration
├── AvailableSensor.cs          # Runtime-discovered LHM sensor
├── HardwareMonitorService.cs   # LibreHardwareMonitor wrapper, sensor discovery
├── MqttBrokerService.cs        # Embedded MQTTnet broker
├── PawnIoHelper.cs             # PawnIO detection and installation
├── Localization.cs             # German/English UI strings
├── SensorPickerForm.cs         # Sensor selection dialog
├── SettingsForm.cs             # Settings dialog
├── app.ico                     # App icon (embedded as resource)
├── app.manifest                # asInvoker, no automatic UAC
├── publish.ps1                 # Single-file publish script
└── Properties/
    ├── launchSettings.json
    └── PublishProfiles/
        ├── SingleFile.pubxml
        ├── SingleFile_NoTrim.pubxml
        └── FrameworkDependent.pubxml
```

---

## Licenses

PulseMQTT itself is released under the **MIT License**.

### Third-party libraries

| Library | License | Source |
|---|---|---|
| [MQTTnet](https://github.com/dotnet/MQTTnet) | MIT | .NET Foundation |
| [MQTTnet.Server](https://github.com/dotnet/MQTTnet) | MIT | .NET Foundation |
| [LibreHardwareMonitorLib](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) | **MPL 2.0** | LibreHardwareMonitor |
| [System.ServiceProcess.ServiceController](https://github.com/dotnet/runtime) | MIT | .NET Foundation |
| .NET 8 Runtime | MIT | Microsoft / .NET Foundation |

**Note on MPL 2.0 (LibreHardwareMonitorLib):**
The Mozilla Public License 2.0 is a weak copyleft license. It applies only to modifications made to the library's own files — not to code that merely *uses* it. PulseMQTT does not modify LibreHardwareMonitorLib.
Full license text: https://www.mozilla.org/en-US/MPL/2.0/

---

## Known Limitations

- **Temperatures/watts require admin** – PawnIO by default only allows administrators driver access. Fix: right-click → *Restart as Administrator*.
- **Sensor names are heuristic** – depending on CPU/GPU manufacturer, LHM sensor names may vary. The sensor picker shows all available sensors with live values for manual selection.
- **Multiple GPUs** – on multi-GPU systems, all GPUs are shown in the sensor picker; sensors from multiple GPUs can be enabled simultaneously (MQTT keys must be unique).
- **Trimmed publish** – WinForms does not fully support IL trimming; the `SingleFile_NoTrim` build is more reliable.
