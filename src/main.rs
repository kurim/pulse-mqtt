// No console window; "--console" opens one.
#![windows_subsystem = "windows"]

mod autostart;
mod config;
mod cpu;
mod fans;
mod gpu;
mod gui;
mod login;
mod migrate;
mod mqtt;
mod pawnio;
mod sensors;
mod serial;
mod setup;
mod storage;
mod system;
mod tray;
mod ui;
mod update;

use std::{
    os::windows::io::IntoRawHandle,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::RecvTimeoutError,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        Console::{AllocConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle},
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories},
        Threading::{
            GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, SetProcessWorkingSetSize,
            WaitForSingleObject,
        },
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
                .filter(|a| !a.starts_with("--after=") && a != "--enable-autostart"),
        )
        .arg(format!("--after={}", std::process::id()))
        .spawn()
        .map(drop)
}

/// The release also offers this program as `pulsemqtt-setup.exe`; under that name it
/// installs itself. Browsers may add to the name, as in "pulsemqtt-setup (1).exe".
fn setup_name(program: &Path) -> bool {
    program.file_stem().is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .contains("setup")
    })
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

/// Hands the memory that is not in use back to Windows, which keeps the tray process
/// small in RAM. Pages that are needed again come back on their own.
fn trim_memory() {
    // SAFETY: the pseudo handle of this process; -1 for both sizes means "empty".
    unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
}

/// Without a console, an error that stops the app would otherwise go unnoticed.
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        // The autostart task retries on its own; a message box would block it.
        if !has_argument("--background") && !has_argument("--silent") {
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

    let setup_file = setup_name(&std::env::current_exe()?) && !has_argument("--install");
    let page = std::env::args()
        .find_map(|a| gui::Page::from_argument(&a))
        .or(setup_file.then_some(gui::Page::Setup));
    let background = has_argument("--background");

    // The installed copy needs admin rights (sensors, settings), and so does removing it.
    let needs_admin = match page {
        Some(page) => page == gui::Page::Uninstall,
        None => autostart::installed() && !background,
    };
    if needs_admin && !autostart::possible() {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        return Ok(setup::elevate(&arguments)?);
    }

    // Settings from earlier versions: 2026.11.3 kept those of the installed copy in
    // %APPDATA%, versions before it kept those of any copy next to the program.
    let user = config::user_dir()?;
    if page == Some(gui::Page::Uninstall) {
        // Nothing to move any more.
    } else if autostart::installed() {
        config::move_settings(&user, &config::data_dir()?)
            .map_err(|e| format!("{e}\n\nStart PulseMQTT as administrator once."))?;
    } else if let Some(folder) = std::env::current_exe()?.parent()
        && config::ours(folder)
    {
        config::move_settings(folder, &user)?;
    }

    // For package managers and scripts.
    if page == Some(gui::Page::Uninstall) && has_argument("--silent") {
        setup::uninstall(has_argument("--purge"))?;
        setup::delete_self_later();
        return Ok(());
    }
    // The windows run as separate processes, mostly started from the tray menu.
    if let Some(page) = page {
        std::process::exit(gui::run(page)?);
    }
    // Started with admin rights by the setup window.
    if has_argument("--install") {
        return Ok(setup::install(has_argument("--enable-autostart"))?);
    }

    // Autostart mode: exit if there is nothing to publish to, so that the task retries.
    let console = has_argument("--console");
    if console {
        open_console();
    }

    if let Some(pid) = std::env::args().find_map(|a| a.strip_prefix("--after=")?.parse().ok()) {
        wait_for_exit(pid);
    }
    if autostart::installed() && tray::running() {
        if !background {
            tray::show_error("PulseMQTT is already running.");
        }
        return Ok(());
    }
    // SAFETY: plain call before any window exists; keeps the tray menu sharp.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    update::clean_up();
    if autostart::installed()
        && autostart::possible()
        && let Err(e) = setup::register()
    {
        eprintln!("Could not register the app: {e}");
    }
    // First start after the .NET app: take over its settings.
    let mut migration = migrate::run();
    if let Some(outcome) = &mut migration {
        if outcome.autostart {
            let taken_over = if autostart::possible() {
                autostart::enable()
            } else {
                Err("administrator rights are missing".into())
            };
            match taken_over {
                Ok(()) => {
                    autostart::remove_old_app();
                    // The new process knows nothing about the migration, so tell the user now.
                    if let Some(message) = &outcome.message {
                        tray::show_error(message);
                    }
                    // Continue with the installed copy, which has the settings now.
                    return Ok(start_again(autostart::program())?);
                }
                Err(e) => outcome.autostart_failed(&e),
            }
        }
        if let Some(message) = outcome.message.clone() {
            eprintln!("{message}");
            // On its own thread, so that publishing starts right away.
            thread::spawn(move || tray::show_error(&message));
        }
    }
    if has_argument("--enable-autostart")
        && autostart::installed()
        && let Err(e) = autostart::enable()
    {
        thread::spawn(move || tray::show_error(&e));
    }
    let mut config = config::Config::load()?;
    if migration.is_some_and(|m| m.block_mqtt) {
        config.mqtt_enabled = false;
    }
    let language = config.language;
    let mut sensors = sensors::Sensors::open();

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
    // The first time shortly after the start, when everything has been set up.
    let mut next_trim = Instant::now() + Duration::from_secs(10);

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
        if Instant::now() >= next_trim {
            trim_memory();
            next_trim = Instant::now() + Duration::from_secs(60);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_setup_file_installs() {
        for name in ["pulsemqtt-setup.exe", "PulseMQTT-Setup (1).exe"] {
            assert!(setup_name(&Path::new(r"C:\Downloads").join(name)), "{name}");
        }
        // A folder name does not count.
        assert!(!setup_name(Path::new(r"C:\setup\pulsemqtt.exe")));
        assert!(!setup_name(&autostart::program()));
    }
}
