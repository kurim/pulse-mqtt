//! Takes over the settings of the earlier .NET app at the first start. A login that
//! cannot be taken over switches MQTT off: the broker must never silently become open.

use crate::{
    autostart,
    config::{Config, Language, MAX_KEY, Sensor, SerialMode},
    login,
};
use serde_json::Value;
use std::{collections::HashSet, io};

/// Additional DPAPI entropy the .NET app used for the password.
const ENTROPY: &[u8] = b"PulseMQTT.MqttPassword.v1";
/// The sensors with fixed ids; drive temperatures depend on the machine.
const SENSORS: [&str; 12] = [
    "cpu_load",
    "cpu_power",
    "cpu_temp",
    "cpu_fan",
    "gpu_load",
    "gpu_core_clock",
    "gpu_power",
    "gpu_temp",
    "gpu_fan",
    "gpu_fan_percent",
    "memory_available",
    "memory_used",
];

#[derive(Debug)]
struct Imported {
    config: Config,
    login: Option<(String, String)>,
    /// The old app was set to start with Windows.
    autostart: bool,
    /// Why parts could not be taken over.
    problems: Vec<String>,
}

pub struct Outcome {
    pub autostart: bool,
    /// For the user, if something needs attention.
    pub message: Option<String>,
    /// Set if MQTT has to stay off for this start although the configuration says otherwise.
    pub block_mqtt: bool,
    language: Language,
}

impl Outcome {
    /// Adds to the message that the user has to switch on the autostart.
    pub fn autostart_failed(&mut self, reason: &str) {
        let hint = self.language.pick(
            "Der Autostart wurde nicht übernommen. Bitte PulseMQTT als Administrator starten, \
             in den Einstellungen „Mit Windows starten“ einschalten und die alte Aufgabe \
             „PulseMQTT“ in der Aufgabenplanung deaktivieren.",
            "The autostart was not taken over. Please start PulseMQTT as administrator, switch \
             on \"Start with Windows\" in the settings and disable the old task \"PulseMQTT\" in \
             the Task Scheduler.",
        );
        let text = format!("{hint}\n({reason})");
        self.message = Some(match self.message.take() {
            Some(message) => format!("{message}\n\n{text}"),
            None => text,
        });
    }
}

/// Does nothing (`None`) without old settings, or once this app has been in use: it has
/// settings, a login or its autostart.
pub fn run() -> Option<Outcome> {
    if Config::exists() || login::exists() {
        return None;
    }
    let path = crate::config::user_dir().ok()?.join("settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(_) => String::new(), // reported as invalid below
    };
    if autostart::enabled() {
        return None;
    }
    let mut imported = import(&text);
    // The login first: `config.json` marks the migration as done.
    let saved = match &imported.login {
        Some((user, password)) => login::save(Some((user, password))),
        None => Ok(()),
    }
    .and_then(|()| imported.config.save());
    let block_mqtt = saved.is_err();
    if let Err(e) = saved {
        // A login left behind would stop the next start from trying again.
        login::save(None).ok();
        imported.problems.push(e);
    }
    let language = imported.config.language;
    let message = (!imported.problems.is_empty()).then(|| {
        format!(
            "{}\n\n{}",
            language.pick(
                "Nicht alle Einstellungen der alten App konnten übernommen werden:",
                "Not all settings of the old app could be taken over:",
            ),
            imported.problems.join("\n")
        )
    });
    Some(Outcome {
        autostart: imported.autostart && !block_mqtt,
        message,
        block_mqtt,
        language,
    })
}

fn import(text: &str) -> Imported {
    let mut config = Config::default();
    let Ok(old @ Value::Object(_)) = serde_json::from_str(text.trim_start_matches('\u{FEFF}'))
    else {
        config.mqtt_enabled = false;
        return Imported {
            config,
            login: None,
            autostart: false,
            problems: vec!["settings.json is invalid. MQTT is switched off.".into()],
        };
    };
    let text_of = |name: &str| old[name].as_str().unwrap_or_default().to_owned();
    let flag = |name: &str, default| old[name].as_bool().unwrap_or(default);

    config.language = match text_of("Language").as_str() {
        "de" => Language::De,
        "en" => Language::En,
        _ => Language::Auto,
    };
    let language = config.language;
    let mut problems = Vec::new();

    config.mqtt_enabled = flag("UseMqtt", true);
    config.auto_update = flag("AutoUpdate", true);
    if let Some(port) = old["MqttPort"].as_u64() {
        // An impossible port becomes 0, which fails validation below.
        config.port = u16::try_from(port).unwrap_or(0);
    }
    if let Some(topic) = old["MqttTopic"].as_str() {
        config.topic = topic.to_owned();
    }
    if let Some(seconds) = old["UpdateIntervalSeconds"].as_f64() {
        config.interval_seconds = seconds.clamp(0.5, 60.0);
    }
    config.serial_port = text_of("SerialPortName");
    config.serial_enabled = flag("UseSerial", false) && !config.serial_port.is_empty();
    if text_of("SerialMode") == "Uart0" {
        config.serial_mode = SerialMode::Uart0;
    }

    let skipped = import_sensors(&old["EnabledSensors"], &mut config);
    if !skipped.is_empty() {
        problems.push(format!(
            "{}: {}",
            language.pick("Nicht unterstützte Sensoren", "Unsupported sensors"),
            skipped.join(", ")
        ));
    }

    let user = text_of("MqttUsername");
    let mut login = None;
    if !user.is_empty() {
        let protected = text_of("MqttPasswordProtected");
        let password = if protected.is_empty() {
            Ok(text_of("MqttPassword")) // very old versions stored it as text
        } else {
            login::decrypt_base64(&protected, ENTROPY)
        };
        match password {
            Ok(password) if !password.is_empty() && !user.contains('\n') => {
                login = Some((user, password));
            }
            _ => {
                config.mqtt_enabled = false;
                problems.push(
                    language
                        .pick(
                            "Das MQTT-Login ist nicht lesbar. MQTT ist ausgeschaltet; bitte \
                             in den Einstellungen neu eingeben und einschalten.",
                            "The MQTT login cannot be read. MQTT is switched off; please enter \
                             it again in the settings and switch MQTT on.",
                        )
                        .to_owned(),
                );
            }
        }
    }

    if let Err(e) = config.validate() {
        // Keep what protects the user, drop the rest.
        config = Config {
            language,
            mqtt_enabled: false,
            ..Config::default()
        };
        problems.push(e);
    }
    Imported {
        config,
        login,
        autostart: flag("StartWithWindows", false),
        problems,
    }
}

/// Publishes what the old app published, under the same keys. Returns the keys of
/// sensors that do not exist here.
fn import_sensors(old: &Value, config: &mut Config) -> Vec<String> {
    let entries: Vec<(&str, &str)> = old
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["IsEnabled"] != false)
        .filter_map(|e| Some((e["Identifier"].as_str()?, e["MqttKey"].as_str()?)))
        .collect();
    // Never configured: keep the defaults.
    if entries.is_empty() {
        return Vec::new();
    }
    for id in SENSORS {
        let off = Sensor {
            enabled: false,
            key: id.to_owned(),
        };
        config.sensors.insert(id.to_owned(), off);
    }
    let (mut used, mut skipped) = (HashSet::new(), Vec::new());
    for (identifier, key) in entries {
        let id = sensor_id(identifier).or_else(|| SENSORS.into_iter().find(|id| *id == key));
        let fits = !key.is_empty() && key.len() <= MAX_KEY;
        match id {
            Some(id) if fits && !config.sensors[id].enabled && used.insert(key) => {
                let on = Sensor {
                    enabled: true,
                    key: key.to_owned(),
                };
                config.sensors.insert(id.to_owned(), on);
            }
            _ => skipped.push(key.to_owned()),
        }
    }
    skipped
}

/// LibreHardwareMonitor identifiers such as "/amdcpu/0/temperature/2".
fn sensor_id(identifier: &str) -> Option<&'static str> {
    let parts: Vec<&str> = identifier.trim_start_matches('/').split('/').collect();
    Some(match parts.as_slice() {
        ["amdcpu" | "intelcpu", _, "load", "0"] => "cpu_load",
        ["amdcpu" | "intelcpu", _, "power", "0"] => "cpu_power",
        ["amdcpu", _, "temperature", "2"] => "cpu_temp",
        [gpu, _, kind, index] if gpu.starts_with("gpu-") => match (*kind, *index) {
            ("load", "0") => "gpu_load",
            ("clock", "0") => "gpu_core_clock",
            ("temperature", "0") => "gpu_temp",
            ("fan", _) => "gpu_fan",
            // The package power has a different index per vendor.
            ("power", "3") if *gpu == "gpu-amd" => "gpu_power",
            ("power", "0") if *gpu != "gpu-amd" => "gpu_power",
            _ => return None,
        },
        ["ram", "data", "0"] => "memory_used",
        ["ram", "data", "1"] => "memory_available",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(extra: Value) -> String {
        let mut base = serde_json::json!({
            "MqttPort": 1884, "MqttTopic": "my/topic", "UpdateIntervalSeconds": 0.25,
            "StartWithWindows": true, "UseSerial": true, "SerialPortName": "COM10",
            "SerialMode": "Uart0", "Language": "de", "AutoUpdate": false,
            "EnabledSensors": [
                {"Identifier": "/amdcpu/0/temperature/2", "MqttKey": "temp", "IsEnabled": true},
                {"Identifier": "/gpu-amd/5/power/3", "MqttKey": "gpu_power", "IsEnabled": true},
                {"Identifier": "/amdcpu/0/clock/1", "MqttKey": "cpu_core1", "IsEnabled": true},
                {"Identifier": "/amdcpu/0/load/0", "MqttKey": "cpu_load", "IsEnabled": false}
            ]
        });
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        base.to_string()
    }

    #[test]
    fn takes_over_settings_sensors_and_login() {
        let protected = login::encrypt_base64("s3cret ü", ENTROPY).unwrap();
        let imported = import(&settings(serde_json::json!({
            "MqttUsername": "esp", "MqttPasswordProtected": protected
        })));
        let config = imported.config;
        assert_eq!(imported.login, Some(("esp".into(), "s3cret ü".into())));
        assert!(config.mqtt_enabled && imported.autostart && !config.auto_update);
        assert_eq!((config.port, config.topic.as_str()), (1884, "my/topic"));
        assert_eq!(config.interval_seconds, 0.5);
        assert!(config.serial_enabled && config.serial_mode == SerialMode::Uart0);
        assert_eq!(config.sensor("cpu_temp").key, "temp");
        assert!(config.sensor("gpu_power").enabled);
        assert!(!config.sensor("cpu_load").enabled);
        assert!(!config.sensor("cpu_fan").enabled);
        assert!(imported.problems[0].ends_with("cpu_core1"));
    }

    #[test]
    fn unreadable_login_or_file_switches_mqtt_off() {
        let broken = [
            settings(serde_json::json!({"MqttUsername": "esp", "MqttPasswordProtected": "AAAA"})),
            settings(serde_json::json!({"MqttUsername": "esp"})),
            settings(serde_json::json!({"MqttPort": 70000})),
            "not json".to_owned(),
            String::new(),
        ];
        for text in broken {
            let imported = import(&text);
            assert!(!imported.config.mqtt_enabled, "{text}");
            assert!(!imported.problems.is_empty(), "{text}");
        }
        // Without a user name the old broker was open, too.
        let open = import(&settings(serde_json::json!({})));
        assert!(open.config.mqtt_enabled && open.login.is_none());
    }
}
