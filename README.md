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

PawnIO has to be installed. Take `pulsemqtt-setup.exe` from the
[releases](https://github.com/kurim/pulse-mqtt/releases) and run it. It installs to
`C:\Program Files\PulseMQTT-Rust`, adds a start menu entry and, if "Start with Windows"
stays on, starts the app with admin rights at every logon. Running a newer setup updates
the installation. `pulsemqtt.exe` from the same release is the same program for use without
installing; the setup is recognised by `setup` in its file name.

To uninstall, use Windows Settings > Apps > Installed apps; settings and login are kept
unless you choose to delete them. Without a window, in an administrator terminal:
`pulsemqtt.exe --uninstall --silent` (add `--purge` to delete settings and login).

The installed app keeps its settings (`config.json`) and the encrypted login
(`login.dat`) next to itself in Program Files, so changing them needs admin rights. A copy
that is not installed keeps them in `%APPDATA%\PulseMQTT`, where every program running
under your account can change them.

At its first start the app takes over the settings and login of the earlier .NET app,
unless it already has a login or autostart of its own. Sensors that only that app offered
are listed in a message. Drive temperatures are published under new keys
(`storage_temp_disk0` and so on); change them in the sensor window if your display expects
the old ones.

To install from source, build with a current Rust toolchain on Windows (MSVC), then start
the copy that is named as the setup:

```powershell
cargo build --release
```

```powershell
Copy-Item target\release\pulsemqtt.exe target\release\pulsemqtt-setup.exe
```

```powershell
.\target\release\pulsemqtt-setup.exe
```

## Use

Right-click the tray icon for settings and sensor selection. Start the program with `--console` to see the sensor values in a console window. Values go to the topic
`pulsemqtt/hwinfo` (QoS 0, not retained) on port 1883.

The broker has no TLS. Set a user name and password: without them everyone on the network
can connect.

## Updates and releases

The app only installs releases signed with the key whose public half is in `update.pub`.

To publish a version, set it in `Cargo.toml` and add its section to `CHANGELOG.md` (it becomes the release notes), merge that into `main`
and push the tag:

```powershell
git tag -s v2026.11.0 -m v2026.11.0
git push origin v2026.11.0
```

The release workflow tests, builds, signs and publishes it as a pre-release. It needs the
repository secrets `MINISIGN_SECRET_KEY` (content of the secret key file) and
`MINISIGN_PASSWORD`.

## License

MIT, see [LICENSE](LICENSE). The embedded PawnIO modules are LGPL-2.1-or-later, see
[modules](modules/README.md).
