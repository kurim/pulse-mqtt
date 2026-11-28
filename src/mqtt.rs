//! Embedded MQTT broker (rumqttd) that publishes the sensor values as one JSON object.
//! QoS 0 and "not retained" match the previous PulseMQTT service.

use crate::config::Config as AppConfig;
use rumqttd::{
    Broker, Config, ConnectionSettings, RouterConfig, ServerSettings,
    local::{LinkRx, LinkTx},
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    thread,
};

pub struct Settings {
    pub port: u16,
    pub topic: String,
    /// Username and password; `None` accepts anonymous clients.
    pub login: Option<(String, String)>,
}

impl Settings {
    /// From the configuration and the saved login. PULSEMQTT_PORT, PULSEMQTT_USER and
    /// PULSEMQTT_PASSWORD take precedence.
    pub fn load(config: &AppConfig) -> Result<Self, String> {
        let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let mut settings = Self::parse(
            config,
            var("PULSEMQTT_PORT"),
            var("PULSEMQTT_USER"),
            var("PULSEMQTT_PASSWORD"),
        )?;
        if settings.login.is_none() {
            settings.login = crate::login::saved()?;
        }
        Ok(settings)
    }

    fn parse(
        config: &AppConfig,
        port: Option<String>,
        user: Option<String>,
        password: Option<String>,
    ) -> Result<Self, String> {
        let port = match port {
            Some(p) => p
                .parse()
                .ok()
                .filter(|&p| p != 0)
                .ok_or_else(|| format!("PULSEMQTT_PORT is not a valid port: {p}"))?,
            None => config.port,
        };
        // Half a login must not silently turn into an open broker.
        let login = match (user, password) {
            (Some(user), Some(password)) => Some((user, password)),
            (None, None) => None,
            _ => return Err("set both PULSEMQTT_USER and PULSEMQTT_PASSWORD, or neither".into()),
        };
        Ok(Self {
            port,
            topic: config.topic.clone(),
            login,
        })
    }
}
pub struct Publisher {
    link: LinkTx,
    topic: String,
    _receiver: LinkRx, // dropping it would close the local link
}

impl Publisher {
    pub fn start(settings: &Settings) -> Result<Self, String> {
        let listen = SocketAddr::from((Ipv4Addr::UNSPECIFIED, settings.port));
        // rumqttd binds in a background thread and only logs failures, so check up front.
        TcpListener::bind(listen).map_err(|e| format!("port {}: {e}", settings.port))?;

        let server = ServerSettings {
            name: "v4".into(),
            listen,
            tls: None,
            next_connection_delay_ms: 1,
            connections: ConnectionSettings {
                connection_timeout_ms: 5000,
                max_payload_size: 20 * 1024,
                max_inflight_count: 100,
                auth: settings.login.clone().map(|login| HashMap::from([login])),
                external_auth: None,
                dynamic_filters: true,
            },
        };
        let mut broker = Broker::new(Config {
            router: RouterConfig {
                max_connections: 64,
                max_outgoing_packet_count: 200,
                // Subscribers only need current values; the router keeps this much history.
                max_segment_size: 64 * 1024,
                max_segment_count: 2,
                ..Default::default()
            },
            v4: Some(HashMap::from([("v4".into(), server)])),
            ..Default::default()
        });
        let (link, receiver) = broker.link("pulsemqtt").map_err(|e| e.to_string())?;
        thread::spawn(move || {
            let result = broker.start();
            eprintln!("MQTT broker stopped: {result:?}");
        });
        Ok(Self {
            link,
            topic: settings.topic.clone(),
            _receiver: receiver,
        })
    }

    pub fn publish(&mut self, payload: String) -> Result<(), String> {
        self.link
            .publish(self.topic.clone(), payload)
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

/// JSON object with one decimal per value; sensors without a value are left out.
pub fn payload<K: Into<String>>(readings: impl IntoIterator<Item = (K, Option<f64>)>) -> String {
    let object: serde_json::Map<_, _> = readings
        .into_iter()
        .filter_map(|(key, value)| Some((key.into(), ((value? * 10.0).round() / 10.0).into())))
        .collect();
    serde_json::Value::Object(object).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumqttc::{Client, ConnectReturnCode, Event, MqttOptions, Packet, QoS};
    use std::time::{Duration, Instant};

    const TOPIC: &str = "test/topic";

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn payload_rounds_and_skips_missing() {
        let json = payload([("cpu_temp", Some(58.125)), ("cpu_fan", None)]);
        assert_eq!(json, r#"{"cpu_temp":58.1}"#);
    }

    #[test]
    fn settings_reject_half_a_login_and_bad_ports() {
        let text = |s: &str| Some(s.to_string());
        let config = AppConfig::default();
        let parse = |port, user, password| Settings::parse(&config, port, user, password);
        assert_eq!(parse(None, None, None).unwrap().port, 1883);
        assert!(parse(None, text("a"), None).is_err());
        assert!(parse(None, None, text("b")).is_err());
        assert!(parse(text("0"), None, None).is_err());
        assert!(parse(text("70000"), None, None).is_err());
    }

    fn settings(port: u16, login: Option<(String, String)>) -> Settings {
        Settings {
            port,
            topic: TOPIC.to_string(),
            login,
        }
    }
    #[test]
    fn client_with_login_receives_published_values() {
        let port = free_port();
        let login = Some(("user".to_string(), "test-only".to_string()));
        let mut publisher = Publisher::start(&settings(port, login)).unwrap();
        thread::sleep(Duration::from_millis(500));

        // Wrong password is refused.
        let mut options = MqttOptions::new("intruder", "127.0.0.1", port);
        options.set_credentials("user", "wrong");
        let (_client, mut connection) = Client::new(options, 10);
        let first = connection.iter().next().unwrap();
        assert!(!matches!(
            first,
            Ok(Event::Incoming(Packet::ConnAck(ack))) if ack.code == ConnectReturnCode::Success
        ));

        let mut options = MqttOptions::new("display", "127.0.0.1", port);
        options.set_credentials("user", "test-only");
        let (client, mut connection) = Client::new(options, 10);
        client.subscribe(TOPIC, QoS::AtMostOnce).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        for event in connection.iter() {
            assert!(Instant::now() < deadline, "nothing received");
            match event.unwrap() {
                Event::Incoming(Packet::SubAck(_)) => {
                    publisher.publish("{\"cpu_load\":1.0}".into()).unwrap();
                }
                Event::Incoming(Packet::Publish(message)) => {
                    assert_eq!(message.topic, TOPIC);
                    assert_eq!(&message.payload[..], b"{\"cpu_load\":1.0}");
                    return;
                }
                _ => {}
            }
        }
        panic!("connection closed");
    }
}
