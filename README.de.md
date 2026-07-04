# PulseMQTT

> 🇬🇧 [English version](README.md)

**PulseMQTT** ist eine schlanke Windows-Tray-App, die PC-Hardwaredaten (CPU/GPU-Last, Temperaturen, Leistungsaufnahme) per MQTT publiziert. Sie richtet sich primär an kleine Displays wie das **CYD (Cheap Yellow Display / ESP32-2432S028)**, funktioniert aber mit jedem MQTT-fähigen Client.

![Tray-Menü](.github/tray_menu.png)

---

## Features

- **Eingebetteter MQTT-Broker** – kein separater Mosquitto-Server nötig; das CYD-Board (oder jeder andere Client) verbindet sich direkt mit PulseMQTT
- **Flexible Sensor-Auswahl** – alle von LibreHardwareMonitor erkannten Sensoren werden aufgelistet; du wählst selbst welche publiziert werden und unter welchem MQTT-Schlüssel
- **Konfigurierbare MQTT-Schlüssel** – Standardwerte sind kompatibel mit der CYD-Firmware (`cpu_load`, `cpu_temp`, `cpu_power`, `gpu_load`, `gpu_temp`, `gpu_power`)
- **Tray-Icon** – läuft unsichtbar im Hintergrund, Live-Werte im Tooltip
- **Keine Adminrechte zum Starten** – optionaler „Als Administrator neu starten"-Eintrag im Menü für volle Sensorzugriff (Temperaturen/Watt erfordern PawnIO-Treiberzugriff)
- **Autostart** – wahlweise per Registry (normale Rechte) oder Task Scheduler (Admin-Modus, kein UAC-Prompt)
- **PawnIO-Assistent** – erkennt ob der Treiber installiert ist und bietet Download+Installation an
- **Single-File-EXE** – optional als portable Einzeldatei publizierbar (kein .NET-Setup nötig)

---

## Voraussetzungen

| Komponente | Version | Hinweis |
|---|---|---|
| Windows | 10 / 11 (x64) | |
| .NET 8 Desktop Runtime | 8.0 oder neuer | Nur bei Framework-abhängigem Build nötig |
| PawnIO-Treiber | aktuell | Für Temperatur- und Leistungssensoren, einmalig als Admin installieren |

### PawnIO

PulseMQTT nutzt [LibreHardwareMonitorLib](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor), das intern den **PawnIO-Treiber** (Nachfolger von WinRing0) für den Low-Level-Hardwarezugriff verwendet.

- **Download:** https://pawnio.eu/
- Die Installation erfordert einmalig Adminrechte (Kernel-Treiber)
- PulseMQTT bietet die Installation beim ersten Start automatisch an
- Ohne PawnIO sind nur CPU-Last-Werte verfügbar (keine Temperaturen, kein Watt)
- Für volle Sensordaten: PulseMQTT über **Rechtsklick → Als Administrator neu starten** erhöht starten

---

## Installation & Start

### Option A – Einfach starten (Framework-abhängig, ~8 MB)

1. [.NET 8 Desktop Runtime](https://dotnet.microsoft.com/download/dotnet/8.0) installieren (einmalig)
2. `PulseMQTT.exe` starten
3. Beim ersten Start: Sensor-Auswahl bestätigen oder anpassen

### Option B – Portable Single-File-EXE (~70 MB, kein .NET nötig)

```powershell
.\publish.ps1 -Mode NoTrim
```

Die EXE liegt anschließend unter `publish_notrim\PulseMQTT.exe` und läuft auf jedem Windows-x64-Rechner ohne zusätzliche Software.

### Option C – Visual Studio Publish

Rechtsklick auf Projekt → **Veröffentlichen** → Profil wählen:

| Profil | Größe | Voraussetzung |
|---|---|---|
| `SingleFile` | ~20 MB | keine (self-contained, getrimmt) |
| `SingleFile_NoTrim` | ~70 MB | keine (self-contained, sicherer) |
| `FrameworkDependent` | ~8 MB | .NET 8 Runtime installiert |

---

## Verwendung

### Erster Start

Beim allerersten Start:
1. PulseMQTT entdeckt automatisch alle verfügbaren Sensoren
2. Sinnvolle Standardwerte werden vorausgewählt (CYD-kompatible MQTT-Schlüssel)
3. Der **Sensor-Picker** öffnet sich zur Bestätigung oder Anpassung

### Tray-Menü

| Eintrag | Funktion |
|---|---|
| **Einstellungen…** | Port, Topic, Update-Intervall, Autostart |
| **Sensoren auswählen…** | Sensor-Picker öffnen (auch Doppelklick aufs Icon) |
| 🔒 **Als Administrator neu starten** | Für volle Temperaturen/Watt-Werte (erscheint nur ohne Admin) |
| ✔ **Läuft als Administrator** | Zeigt erhöhten Modus an (ausgegraut) |
| ✔ **PawnIO installiert** | PawnIO erkannt (ausgegraut) |
| **PawnIO installieren…** | Download + Installation des Treibers (erscheint nur wenn fehlt) |
| **Beenden** | App beenden |

### Sensor-Picker

Öffnet sich über **Rechtsklick → Sensoren auswählen…** oder **Doppelklick** aufs Tray-Icon.

- Alle erkannten Sensoren werden tabellarisch angezeigt (Hardware, Typ, Name, aktueller Wert)
- **Häkchen** setzt den Sensor als aktiv
- **MQTT-Schlüssel** (gelbe Spalte) ist frei editierbar – dieser Name erscheint im JSON-Payload
- **Standardwerte** stellt die CYD-kompatible Vorauswahl wieder her
- Live-Werte aktualisieren sich alle 2 Sekunden während der Dialog offen ist

### Einstellungen

| Feld | Standard | Beschreibung |
|---|---|---|
| MQTT-Port | `1883` | Port des eingebetteten Brokers |
| MQTT-Topic | `pulsemqtt/hwinfo` | Topic unter dem der Payload publiziert wird |
| Update-Intervall | `2,0 s` | Wie oft neue Werte gesendet werden |
| Mit Windows starten | aus | Autostart (Registry oder Task Scheduler) |

Gespeichert unter `%AppData%\PulseMQTT\settings.json`.

---

## MQTT-Payload

PulseMQTT publiziert ein JSON-Objekt mit den vom Benutzer konfigurierten Feldern.  
Standardauswahl (CYD-Firmware-kompatibel):

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

Die Feldnamen entsprechen den MQTT-Schlüsseln, die du im Sensor-Picker vergibst. Eigene Schlüssel sind möglich, müssen dann aber auch in der Firmware/im Display-Client angepasst werden.

### CYD-Board verbinden

Im CYD-Webinterface (Captive Portal oder `http://<CYD-IP>`):

| Feld | Wert |
|---|---|
| `mqtt_host` | LAN-IP des PCs auf dem PulseMQTT läuft |
| `mqtt_port` | `1883` (oder wie konfiguriert) |
| `mqtt_topic` | `pulsemqtt/hwinfo` (oder wie konfiguriert) |

---

## Autostart

| Modus | Methode | UAC beim Start |
|---|---|---|
| Normaler Benutzer | `HKCU\...\Run` Registry-Eintrag | keiner |
| Administrator | Windows Task Scheduler (`/rl highest`) | keiner |

Wenn PulseMQTT als Admin läuft und „Mit Windows starten" aktiviert ist, wird ein Task-Scheduler-Task angelegt, der beim Login ohne UAC-Prompt erhöht startet.

---

## Aus dem Quellcode bauen

**Voraussetzungen:**
- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)
- Windows (WinForms-Ziel)

```powershell
git clone https://github.com/<dein-username>/PulseMQTT.git
cd PulseMQTT
dotnet restore
dotnet run --project PulseMQTT
```

**Single-File-EXE erstellen:**

```powershell
cd PulseMQTT
.\publish.ps1                    # NoTrim ~70 MB (empfohlen)
.\publish.ps1 -Mode Fx           # Framework-abhängig ~8 MB
```

---

## Projektstruktur

```
PulseMQTT.sln
PulseMQTT/
├── Program.cs                  # Einstiegspunkt, Single-Instance-Check
├── TrayAppContext.cs           # Tray-Icon, Menü, Poll-Timer, Orchestrierung
├── AppSettings.cs              # Einstellungen (JSON unter %AppData%)
├── SensorEntry.cs              # Gespeicherte Sensor-Konfiguration
├── AvailableSensor.cs          # Zur Laufzeit entdeckter LHM-Sensor
├── HardwareMonitorService.cs   # LibreHardwareMonitor-Wrapper, Sensor-Discovery
├── MqttBrokerService.cs        # Eingebetteter MQTTnet-Broker
├── PawnIoHelper.cs             # PawnIO-Erkennung und Installation
├── SensorPickerForm.cs         # Sensor-Auswahl-Dialog
├── SettingsForm.cs             # Einstellungen-Dialog
├── app.ico                     # App-Icon (eingebettet als Resource)
├── app.manifest                # asInvoker, kein automatisches UAC
├── publish.ps1                 # Single-File-Publish-Skript
└── Properties/
    ├── launchSettings.json
    └── PublishProfiles/
        ├── SingleFile.pubxml
        ├── SingleFile_NoTrim.pubxml
        └── FrameworkDependent.pubxml
```

---

## Lizenzen

PulseMQTT selbst ist unter der **MIT License** veröffentlicht.

### Verwendete Bibliotheken

| Bibliothek | Lizenz | Quelle |
|---|---|---|
| [MQTTnet](https://github.com/dotnet/MQTTnet) | MIT | .NET Foundation |
| [MQTTnet.Server](https://github.com/dotnet/MQTTnet) | MIT | .NET Foundation |
| [LibreHardwareMonitorLib](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) | **MPL 2.0** | LibreHardwareMonitor |
| [System.ServiceProcess.ServiceController](https://github.com/dotnet/runtime) | MIT | .NET Foundation |
| .NET 8 Runtime | MIT | Microsoft / .NET Foundation |

**Hinweis zu MPL 2.0 (LibreHardwareMonitorLib):**  
Die Mozilla Public License 2.0 ist eine schwache Copyleft-Lizenz. Sie gilt nur für Änderungen an den Dateien der Bibliothek selbst – nicht für den PulseMQTT-Code, der sie *verwendet*. PulseMQTT nimmt keine Änderungen an LibreHardwareMonitorLib vor.  
Vollständiger Lizenztext: https://www.mozilla.org/en-US/MPL/2.0/

---

## Bekannte Einschränkungen

- **Temperaturen/Watt nur als Admin** – PawnIO erlaubt standardmäßig nur Administratoren den Treiberzugriff. Lösung: Rechtsklick → *Als Administrator neu starten*.
- **Sensor-Namen sind Heuristiken** – je nach CPU/GPU-Hersteller können LHM-Sensornamen abweichen. Der Sensor-Picker zeigt alle verfügbaren Sensoren mit Echtwerten zur manuellen Auswahl.
- **Mehrere GPUs** – bei Multi-GPU-Systemen werden alle GPUs im Sensor-Picker angezeigt; es können Sensoren von mehreren GPUs gleichzeitig aktiviert werden (MQTT-Schlüssel müssen dann unterschiedlich sein).
- **Trimmed Publish** – WinForms unterstützt IL-Trimming nicht vollständig; der `SingleFile_NoTrim`-Build ist zuverlässiger.
