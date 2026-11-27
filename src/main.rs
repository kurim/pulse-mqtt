mod config;
mod cpu;
mod fans;
mod gpu;
mod gui;
mod login;
mod mqtt;
mod pawnio;
mod sensors;
mod serial;
mod storage;
mod tray;
mod update;

use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc::RecvTimeoutError,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        Console::FreeConsole,
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories},
        Threading::{CREATE_NEW_CONSOLE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    },
    UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
};

/// Waits up to 10 s for the process that restarted us, so that the broker port is free.
fn wait_for_exit(pid: u32) {
    // SAFETY: the handle is checked and closed.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !process.is_null() {
            WaitForSingleObject(process, 10_000);
            CloseHandle(process);
        }
    }
}

/// Starts a new copy with the same arguments that takes over once this one has exited.
fn start_again(background: bool) -> std::io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg(format!("--after={}", std::process::id()));
    if background {
        command
            .arg("--background")
            // There is no console whose handles could be inherited.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    } else {
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    command.spawn().map(drop)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Driver DLLs are loaded by bare name; never pick one up from the app or working directory.
    // SAFETY: plain Win32 call without pointers, made before any library is loaded.
    let restricted = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
    assert!(restricted != 0, "could not restrict the DLL search path");

    // The settings windows run as separate processes, started from the tray menu.
    if let Some(page) = std::env::args().find_map(|a| gui::Page::from_argument(&a)) {
        if gui::run(page)? {
            std::process::exit(gui::SAVED);
        }
        return Ok(());
    }

    // Autostart mode: no console window and no output.
    let background = std::env::args().any(|arg| arg == "--background");
    if background {
        // SAFETY: plain Win32 call without arguments.
        unsafe { FreeConsole() };
    }

    if let Some(pid) = std::env::args().find_map(|a| a.strip_prefix("--after=")?.parse().ok()) {
        wait_for_exit(pid);
    }
    // SAFETY: plain call before any window exists; keeps the tray menu sharp.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    update::clean_up();
    let config = config::Config::load()?;
    let language = config.language;
    let mut sensors = sensors::Sensors::open()?;

    let mut status = Vec::new();
    let mut broker = None;
    if config.mqtt_enabled {
        let settings = mqtt::Settings::load(&config)?;
        match mqtt::Publisher::start(&settings) {
            Ok(publisher) => {
                let access = if settings.login.is_some() {
                    "login required"
                } else {
                    "anonymous access, everyone on the network can connect"
                };
                if !background {
                    println!(
                        "MQTT broker on port {}, topic {} ({access})",
                        settings.port, settings.topic
                    );
                }
                broker = Some(publisher);
                status.push(language.pick("MQTT aktiv", "MQTT active"));
            }
            // Without a broker or serial port the background process is useless:
            // exit so the task retries.
            Err(e) if background && !config.serial_enabled => return Err(e.into()),
            Err(e) => {
                eprintln!("MQTT broker unavailable: {e}");
                status
                    .push(language.pick("MQTT-Broker nicht verfügbar", "MQTT broker unavailable"));
            }
        }
    }
    let mut serial = config.serial_enabled.then(|| {
        status.push(language.pick("Seriell aktiv", "Serial active"));
        serial::Serial::new(config.serial_port.clone(), config.serial_mode)
    });
    if status.is_empty() {
        status.push(language.pick("Senden ist ausgeschaltet", "Sending is switched off"));
    }

    let events = tray::spawn(
        format!("PulseMQTT - {}", status.join(", ")),
        language,
        config.auto_update,
    );
    let interval = Duration::from_secs_f64(config.interval_seconds);

    loop {
        // CPU load and power are averages over this pause.
        match events.recv_timeout(interval) {
            Ok(tray::Event::Quit) => return Ok(()),
            Ok(tray::Event::Reload) => return Ok(start_again(background)?),
            Err(RecvTimeoutError::Timeout) => {}
            // The tray thread is gone; keep publishing.
            Err(RecvTimeoutError::Disconnected) => thread::sleep(interval),
        }

        let readings = sensors.read();
        if !background {
            println!();
            for r in &readings {
                match r.value {
                    Some(v) => println!("{:<22}{v:>8.1} {}", r.id, r.unit()),
                    None => println!("{:<22}{:>8}", r.id, "n/a"),
                }
            }
        }

        let json = mqtt::payload(readings.iter().filter_map(|r| {
            let sensor = config.sensor(&r.id);
            sensor.enabled.then_some((sensor.key, r.value))
        }));
        if let Some(serial) = &mut serial
            && let Err(e) = serial.send(&json)
        {
            eprintln!("Serial output failed: {e}");
        }
        if let Some(broker) = &mut broker
            && let Err(e) = broker.publish(json)
        {
            eprintln!("MQTT publish failed: {e}");
        }
    }
}
