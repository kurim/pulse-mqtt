// No console window; "--console" opens one.
#![windows_subsystem = "windows"]

mod autostart;
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
    os::windows::io::IntoRawHandle, path::PathBuf, process::Command, sync::mpsc::RecvTimeoutError,
    thread, time::Duration,
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        Console::{AllocConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle},
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories},
        Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
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
fn start_again(program: PathBuf) -> std::io::Result<()> {
    Command::new(program)
        .args(
            std::env::args()
                .skip(1)
                .filter(|a| !a.starts_with("--after=")),
        )
        .arg(format!("--after={}", std::process::id()))
        .spawn()
        .map(drop)
}

/// Opens a console window for the sensor values and error messages.
fn open_console() {
    // SAFETY: plain Win32 call; it fails harmlessly if there already is a console.
    unsafe { AllocConsole() };
    // Handles passed in by a parent process would otherwise stay in place.
    if let Ok(console) = std::fs::File::options().write(true).open("CONOUT$") {
        let handle = console.into_raw_handle();
        // SAFETY: the handle stays open for the life of the process.
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
}

fn has_argument(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

/// Without a console, an error that stops the app would otherwise go unnoticed.
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        // The autostart task retries on its own; a message box would block it.
        if !has_argument("--background") {
            tray::show_error(&e.to_string());
        }
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Driver DLLs are loaded by bare name; never pick one up from the app or working directory.
    // SAFETY: plain Win32 call without pointers, made before any library is loaded.
    let restricted = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
    assert!(restricted != 0, "could not restrict the DLL search path");

    // The settings windows run as separate processes, started from the tray menu.
    if let Some(page) = std::env::args().find_map(|a| gui::Page::from_argument(&a)) {
        std::process::exit(gui::run(page)?);
    }

    // Autostart mode: exit if there is nothing to publish to, so that the task retries.
    let background = has_argument("--background");
    let console = has_argument("--console");
    if console {
        open_console();
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
                if console {
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
            Ok(tray::Event::Reload) => return Ok(start_again(std::env::current_exe()?)?),
            Ok(tray::Event::Switch) => return Ok(start_again(autostart::program())?),
            Err(RecvTimeoutError::Timeout) => {}
            // The tray thread is gone; keep publishing.
            Err(RecvTimeoutError::Disconnected) => thread::sleep(interval),
        }

        let readings = sensors.read();
        if console {
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
