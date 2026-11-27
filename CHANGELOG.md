# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions of the Rust app follow `year.month.patch`.

## [Unreleased]

## [2026.11.0] - 2026-11-27

First release of the rewrite in Rust. It replaces the .NET app, whose last release is
[v0.9.2](https://github.com/kurim/pulse-mqtt/releases/tag/v0.9.2) and whose code is on the
[`dotnet` branch](https://github.com/kurim/pulse-mqtt/tree/dotnet).

### Added

- Own sensor reading without LibreHardwareMonitor: CPU load and memory, AMD Ryzen
  temperature and package power and Nuvoton NCT6799D fan speeds through PawnIO, GPU
  values through AMD ADLX or NVIDIA NVML (NVIDIA untested), and drive temperatures.
- Default MQTT values for CPU fan, GPU fan and core clock, memory and drive temperatures.
- Embedded MQTT broker and USB/serial output with the same default topic and the same
  keys for CPU and GPU load, temperature and power.
- Tray menu in the Windows 11 style, settings window and sensor window in German and
  English.
- `install.ps1`, which installs to Program Files and starts the app with admin rights at
  logon.
- Self-update that only installs releases with a valid minisign signature.

### Changed

- Settings are stored in `config.json` next to the program, the encrypted login in
  `login.dat`. Settings of the .NET app are not imported.
- Autostart uses a scheduled task created by `install.ps1`.
- Releases are published as pre-releases, so that the .NET app does not install them.

### Removed

- The free choice among all LibreHardwareMonitor sensors, such as values per CPU core.
  The sensor window offers a fixed list.
- CPU temperature and power on Intel processors.
- Fan speeds on boards without a Nuvoton NCT6799D.
- The choice between sources for drive sensors.

[Unreleased]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.0...HEAD
[2026.11.0]: https://github.com/kurim/pulse-mqtt/releases/tag/v2026.11.0
