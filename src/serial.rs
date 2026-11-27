//! The same JSON lines over a serial port at 115200 baud, reconnecting on failure.

use crate::config::SerialMode;
use serialport::SerialPort;
use std::{
    io::Write,
    time::{Duration, Instant},
};

const RETRY: Duration = Duration::from_secs(5);

pub struct Serial {
    name: String,
    mode: SerialMode,
    port: Option<Box<dyn SerialPort>>,
    /// Earliest next write after opening, or next attempt to open.
    wait_until: Instant,
}

impl Serial {
    pub fn new(name: String, mode: SerialMode) -> Self {
        Self {
            name,
            mode,
            port: None,
            wait_until: Instant::now(),
        }
    }

    /// Sends one line; does nothing while waiting for the device.
    pub fn send(&mut self, json: &str) -> Result<(), String> {
        let now = Instant::now();
        if now < self.wait_until {
            return Ok(());
        }
        let Some(port) = &mut self.port else {
            let usb = self.mode == SerialMode::UsbSerialJtag;
            let opened = serialport::new(&self.name, 115_200)
                .timeout(Duration::from_secs(1))
                .dtr_on_open(usb)
                .open();
            return match opened {
                Ok(port) => {
                    self.port = Some(port);
                    // The built-in USB needs a moment before it accepts data.
                    self.wait_until = now + Duration::from_millis(if usb { 1500 } else { 0 });
                    Ok(())
                }
                Err(e) => {
                    self.wait_until = now + RETRY;
                    Err(format!("{}: {e}", self.name))
                }
            };
        };
        port.write_all(format!("{json}\n").as_bytes()).map_err(|e| {
            self.port = None;
            self.wait_until = now + RETRY;
            format!("{}: {e}", self.name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_open_is_reported_once_then_waits() {
        let mut serial = Serial::new("COM_DOES_NOT_EXIST".into(), SerialMode::Uart0);
        assert!(serial.send("{}").is_err());
        assert!(serial.send("{}").is_ok());
    }
}
