# PulseMQTT

Windows tray app that reads PC sensors and publishes them as JSON through its own MQTT
broker or over USB/serial, for example to the
[ESP32 hardware monitor](https://github.com/kurim/pulse-esp32-hwmonitor).

Rust rewrite of the earlier .NET app, which stays available on the
[`dotnet` branch](https://github.com/kurim/pulse-mqtt/tree/dotnet).

## Sensors

| Value | Source |
|---|---|
| CPU load, memory | Windows |
| CPU temperature and power (AMD Ryzen) | [PawnIO](https://pawnio.eu/) |
| CPU and case fans (Nuvoton NCT6799D only) | PawnIO |
| GPU load, clock, power, temperature, fan | AMD ADLX or NVIDIA NVML |
| Drive temperature | Windows |

Intel CPU temperature and power are not supported yet, and the NVIDIA path is untested.
The `cpu_fan` header was verified on an ASUS ROG STRIX B650E-I only; on other boards
select the matching `fan_header_N` in the sensor window.

## Install

PawnIO has to be installed. Build with a current Rust toolchain on Windows (MSVC), then
run the installer in an administrator PowerShell:

```powershell
cargo build --release
```

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

It copies the app to `C:\Program Files\PulseMQTT-Rust` and starts it with admin rights at
every logon. `-ReplaceOld` switches off the old .NET app, `-Uninstall` removes this one.

## Use

Right-click the tray icon for settings and sensor selection. Values go to the topic
`pulsemqtt/hwinfo` (QoS 0, not retained) on port 1883.

The broker has no TLS. Set a user name and password: without them everyone on the network
can connect.

## Updates and releases

The app only installs releases signed with the key whose public half is in `update.pub`.
`release.ps1` builds, signs and publishes a version; see the notes inside it.

## License

MIT, see [LICENSE](LICENSE). The embedded PawnIO modules are LGPL-2.1-or-later, see
[modules](modules/README.md).
