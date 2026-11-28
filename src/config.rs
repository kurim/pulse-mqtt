//! Settings edited in the windows, stored as `config.json`. The login is next to it,
//! encrypted, see `login`. The installed copy keeps both next to the program, where only
//! admins can change them; otherwise they are in `%APPDATA%\PulseMQTT`.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io,
    path::{Path, PathBuf},
};
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

/// The ESP32 display stores keys in 24 bytes including the terminating NUL.
pub const MAX_KEY: usize = 23;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Auto,
    De,
    En,
}

impl Language {
    pub fn german(self) -> bool {
        match self {
            // SAFETY: no arguments. The low 10 bits are the primary language, 0x07 = German.
            Self::Auto => (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == 0x07,
            Self::De => true,
            Self::En => false,
        }
    }

    pub fn pick<'a>(self, german: &'a str, english: &'a str) -> &'a str {
        if self.german() { german } else { english }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum SerialMode {
    /// Built-in USB of the ESP32-C3/S3; needs DTR.
    #[default]
    UsbSerialJtag,
    /// USB-serial converter chip; DTR would reset the board.
    Uart0,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Sensor {
    pub enabled: bool,
    /// Field name in the published JSON.
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub mqtt_enabled: bool,
    pub port: u16,
    pub topic: String,
    pub serial_enabled: bool,
    pub serial_port: String,
    pub serial_mode: SerialMode,
    pub interval_seconds: f64,
    pub language: Language,
    /// Install new releases without asking, checked at start and once a day.
    pub auto_update: bool,
    /// By sensor id; sensors without an entry use their defaults.
    pub sensors: BTreeMap<String, Sensor>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt_enabled: true,
            port: 1883,
            topic: "pulsemqtt/hwinfo".into(),
            serial_enabled: false,
            serial_port: String::new(),
            serial_mode: SerialMode::default(),
            interval_seconds: 1.0,
            language: Language::default(),
            auto_update: true,
            sensors: BTreeMap::new(),
        }
    }
}

/// Sensors beyond the basic set: off by default and only listed on request.
pub fn extra(id: &str) -> bool {
    const EXTRA: [&str; 6] = [
        "gpu_hotspot_temp",
        "gpu_memory_clock",
        "gpu_memory_used",
        "gpu_voltage",
        "gpu_board_power",
        "memory_total",
    ];
    // Numbered ones: logical processors, drives and fan headers.
    const PREFIXES: [&str; 3] = ["cpu_load_", "disk_usage_", "fan_header_"];
    EXTRA.contains(&id) || PREFIXES.iter().any(|p| id.starts_with(p))
}

impl Config {
    /// The saved entry, or the default: the id as key; extra sensors are off.
    pub fn sensor(&self, id: &str) -> Sensor {
        self.sensors.get(id).cloned().unwrap_or_else(|| Sensor {
            enabled: !extra(id),
            key: id.to_owned(),
        })
    }

    fn path() -> Result<PathBuf, String> {
        Ok(data_dir()?.join("config.json"))
    }
    pub fn exists() -> bool {
        Self::path().is_ok_and(|path| path.exists())
    }

    /// Defaults if there is no file; an invalid file is an error.
    pub fn load() -> Result<Self, String> {
        let path = Self::path()?;
        let fail = |e: String| format!("{}: {e}", path.display());
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(fail(e.to_string())),
        };
        let config: Self = serde_json::from_str(&text).map_err(|e| fail(e.to_string()))?;
        config.validate().map_err(fail)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        write_replacing(&Self::path()?, &json)
    }

    pub fn validate(&self) -> Result<(), String> {
        let language = self.language;
        let error = |de: &str, en: &str| Err(language.pick(de, en).to_owned());
        if self.port == 0 {
            return error("Der Port darf nicht 0 sein.", "The port must not be 0.");
        }
        if self.topic.is_empty() || self.topic.contains(['+', '#', '\0']) {
            return error(
                "Das Topic darf nicht leer sein und weder + noch # enthalten.",
                "The topic must not be empty or contain + or #.",
            );
        }
        if !(0.5..=60.0).contains(&self.interval_seconds) {
            return error(
                "Das Intervall muss zwischen 0,5 und 60 s liegen.",
                "The interval must be between 0.5 and 60 s.",
            );
        }
        // Other programs of the user can edit the file; nothing but a COM port is opened.
        let com_port = self
            .serial_port
            .strip_prefix("COM")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if self.serial_enabled && !com_port {
            return error("Wähle einen seriellen Port.", "Select a serial port.");
        }
        let mut keys = HashSet::new();
        for sensor in self.sensors.values().filter(|s| s.enabled) {
            if sensor.key.is_empty() || sensor.key.len() > MAX_KEY || !keys.insert(&sensor.key) {
                return Err(format!(
                    "{}: \"{}\"",
                    language.pick(
                        "MQTT-Schlüssel müssen eindeutig und 1 bis 23 Zeichen lang sein",
                        "MQTT keys must be unique and 1 to 23 characters long"
                    ),
                    sensor.key
                ));
            }
        }
        Ok(())
    }
}

/// The folder for settings and login.
pub fn data_dir() -> Result<PathBuf, String> {
    if crate::autostart::installed() {
        let program = crate::autostart::program();
        let folder = program.parent().ok_or("no program folder")?;
        Ok(folder.to_owned())
    } else {
        user_dir()
    }
}

/// `%APPDATA%\PulseMQTT`, also where the .NET app kept its settings.
pub fn user_dir() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(path).join("PulseMQTT"))
        .ok_or_else(|| "APPDATA is not set".to_owned())
}

/// Whether `folder` holds a `config.json` written by this app. Other programs use that
/// name too, for example in a download folder.
pub fn ours(folder: &Path) -> bool {
    std::fs::read(folder.join("config.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<serde_json::Value>(&text).ok())
        .is_some_and(|v| v.get("mqtt_enabled").is_some() && v.get("topic").is_some())
}

/// Moves settings that an earlier version kept in `from`, as long as `to` has no
/// `config.json` of its own. A file that already exists in `to` wins.
pub fn move_settings(from: &Path, to: &Path) -> Result<(), String> {
    // shortcut: until the first save, an installed copy without settings also takes files
    // that another program of the user put there; drop this once 2026.11.3 is history.
    if to.join("config.json").exists() {
        return Ok(());
    }
    for name in ["login.dat", "config.json"] {
        let (source, target) = (from.join(name), to.join(name));
        if source.is_file() && !target.exists() {
            let content =
                std::fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
            write_replacing(&target, &content)?;
            // Without admin rights an old file in Program Files stays; it is ignored.
            std::fs::remove_file(source).ok();
        }
    }
    Ok(())
}

/// Writes via a temporary file, so a failure cannot leave a half-written one behind.
pub fn write_replacing(path: &Path, content: &[u8]) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, content)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let config: Config = serde_json::from_str(r#"{"port": 1884}"#).unwrap();
        assert_eq!(config.port, 1884);
        assert_eq!(config.topic, "pulsemqtt/hwinfo");
        assert!(config.sensor("cpu_temp").enabled);
        assert!(!config.sensor("fan_header_1").enabled);
        assert!(!config.sensor("cpu_load_1").enabled);
        assert!(!config.sensor("gpu_voltage").enabled);
        assert!(config.sensor("cpu_load").enabled && config.sensor("gpu_power").enabled);
    }

    #[test]
    fn moves_settings_without_replacing_newer_ones() {
        let root = std::env::temp_dir().join(format!("pulsemqtt-move-{}", std::process::id()));
        let (from, to) = (root.join("from"), root.join("to"));
        for folder in [&from, &to] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(from.join("login.dat"), b"older").unwrap();
        std::fs::write(from.join("config.json"), b"config").unwrap();
        std::fs::write(to.join("login.dat"), b"in use").unwrap();
        move_settings(&from, &to).unwrap();
        assert_eq!(std::fs::read(to.join("login.dat")).unwrap(), b"in use");
        assert_eq!(std::fs::read(to.join("config.json")).unwrap(), b"config");
        assert!(!from.join("config.json").exists());
        // With settings of its own, nothing is taken any more.
        std::fs::remove_file(to.join("login.dat")).unwrap();
        move_settings(&from, &to).unwrap();
        assert!(!to.join("login.dat").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_our_config_is_recognised() {
        let root = std::env::temp_dir().join(format!("pulsemqtt-ours-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(!ours(&root));
        for (text, expected) in [
            (r#"{"name": "another program"}"#.to_owned(), false),
            ("not json".to_owned(), false),
            (serde_json::to_string(&Config::default()).unwrap(), true),
        ] {
            std::fs::write(root.join("config.json"), &text).unwrap();
            assert_eq!(ours(&root), expected, "{text}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_settings() {
        assert!(Config::default().validate().is_ok());
        let broken: [fn(&mut Config); 5] = [
            |c| c.port = 0,
            |c| c.topic = "a/#".into(),
            |c| c.interval_seconds = 0.0,
            |c| {
                c.serial_enabled = true;
                c.serial_port = r"C:\file".into();
            },
            |c| {
                for id in ["a", "b"] {
                    let sensor = Sensor {
                        enabled: true,
                        key: "same".into(),
                    };
                    c.sensors.insert(id.into(), sensor);
                }
            },
        ];
        for (index, change) in broken.iter().enumerate() {
            let mut config = Config::default();
            change(&mut config);
            assert!(config.validate().is_err(), "case {index}");
        }
        let mut config = Config::default();
        let long = Sensor {
            enabled: true,
            key: "x".repeat(MAX_KEY + 1),
        };
        config.sensors.insert("a".into(), long);
        assert!(config.validate().is_err());
    }
}
