# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions of the Rust app follow `year.month.patch`.

## [Unreleased]

## [2026.11.4] - 2026-11-28

### Added

- `pulsemqtt-setup.exe` installs or updates the app. The installed app appears in the start
  menu and under Installed apps in Windows Settings, where it can be uninstalled,
  optionally with its settings and login. `pulsemqtt.exe` still runs without installing.

### Changed

- The installed app asks for administrator rights when started by hand, and only one copy
  of it runs.
- The installed app keeps `config.json` and `login.dat` in Program Files again, where only
  admins can change them. Files that 2026.11.3 moved to `%APPDATA%\PulseMQTT` are moved
  back at the first start. A copy that is not installed keeps using that folder.
- CPU load, memory and drive usage are read directly from Windows, without the `sysinfo`
  library. Only fixed drives are asked.
- The settings of the .NET app are no longer taken over if a login or the autostart of this
  app already exists.

### Removed

- `install.ps1`.

### Fixed

- A `config.json` of another program next to the downloaded app is no longer moved away.
- A message appears if the autostart of the .NET app cannot be taken over.
- A drive mounted into a folder no longer appears under the letter of the other drive.

## [2026.11.3] - 2026-11-28

### Added

- More sensors, listed with `Show all detected` and off by default: load per logical
  processor, GPU hotspot temperature, memory clock, memory used, voltage and board power,
  total memory and usage of fixed drives.
- The first start takes over the settings of the .NET app: port, topic, interval, serial
  output, language, selected sensors with their keys, and the MQTT login. Run as
  administrator it also replaces the old autostart. MQTT stays off if the login cannot be
  read.

### Changed

- The settings and sensor windows are drawn with Windows GDI. They need about 2 to 8 MB
  of memory instead of about 60 MB, and the program shrinks to about 3.4 MB.
- `config.json` and `login.dat` are stored in `%APPDATA%\PulseMQTT` instead of next to
  the program. Existing files are moved there at the first start.
- The tray process hands unused memory back to Windows once a minute and shows about
  1 MB instead of 8 MB of private memory in Task Manager.

## [2026.11.2] - 2026-11-28

### Changed

- `Start with Windows` can be switched on in the settings window when the app runs as
  administrator. It copies itself to Program Files; `install.ps1` is no longer needed.

## [2026.11.1] - 2026-11-27

### Changed

- The program is smaller (about 7.6 MB instead of 11.7 MB). It uses the Windows fonts.
- The MQTT broker keeps less message history in memory.
- The title bar of the windows has the same colour as the window (Windows 11).
- Release notes are taken from this changelog.
- In the sensor window only the table scrolls.

### Fixed

- No console window opens at start. Use `--console` to see the sensor values.

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

[Unreleased]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.4...HEAD
[2026.11.4]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.3...v2026.11.4
[2026.11.3]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.2...v2026.11.3
[2026.11.2]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.1...v2026.11.2
[2026.11.1]: https://github.com/kurim/pulse-mqtt/compare/v2026.11.0...v2026.11.1
[2026.11.0]: https://github.com/kurim/pulse-mqtt/releases/tag/v2026.11.0
