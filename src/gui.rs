//! The settings and sensor windows, run as their own process.

use crate::{
    autostart,
    config::{self, Config, Language, MAX_KEY, Sensor, SerialMode},
    login,
    sensors::{Group, Kind, Reading, Sensors},
    setup, tray,
    ui::{self, BACKGROUND, CARD, ERROR, Event, Font, Look, MUTED, TEXT, Window},
    update,
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        WindowsAndMessaging::{BN_CLICKED, EN_CHANGE, ES_NUMBER, ES_PASSWORD},
    },
};

/// Exit code of a window process that saved changes.
pub const SAVED: i32 = 10;
/// Saved, and the app was just set up to start with Windows: continue with that copy.
pub const INSTALLED: i32 = 11;

// Enter and Escape send the first two ids.
const SAVE: u16 = 1;
const CANCEL: u16 = 2;
const DEFAULTS: u16 = 3;
const SHOW_ALL: u16 = 4;
const UNINSTALL: u16 = 5;
/// First of the filter buttons.
const FILTER: u16 = 10;
const FOOTER: i32 = 68;
/// Top of the sensor table.
const TABLE: i32 = 232;
const FILTERS: [Option<Group>; 5] = [
    None,
    Some(Group::Cpu),
    Some(Group::Gpu),
    Some(Group::Memory),
    Some(Group::Drive),
];
const LANGUAGES: [Language; 3] = [Language::Auto, Language::De, Language::En];
const MODES: [SerialMode; 2] = [SerialMode::UsbSerialJtag, SerialMode::Uart0];

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Settings,
    Sensors,
    Setup,
    Uninstall,
}

impl Page {
    pub fn from_argument(argument: &str) -> Option<Self> {
        match argument {
            "--settings" => Some(Self::Settings),
            "--sensors" => Some(Self::Sensors),
            "--setup" => Some(Self::Setup),
            "--uninstall" => Some(Self::Uninstall),
            _ => None,
        }
    }
}

/// What the window edits and stores.
struct App {
    config: Config,
    exit: i32,
    /// What is stored; `None` if it could not be read.
    stored_login: Option<Option<(String, String)>>,
    autostart_before: bool,
}

/// The values of the settings window that are not part of the configuration.
struct Entered {
    user: String,
    password: String,
    autostart: bool,
}

impl App {
    /// Also returns what went wrong while loading; saving then replaces a broken file.
    fn load() -> (Self, Option<String>) {
        let (config, error) = match Config::load() {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(e)),
        };
        let app = Self {
            config,
            exit: 0,
            stored_login: None,
            autostart_before: false,
        };
        (app, error)
    }

    fn t<'a>(&self, german: &'a str, english: &'a str) -> &'a str {
        self.config.language.pick(german, english)
    }

    /// Saves the configuration and, for the settings window, login and autostart.
    fn save(&mut self, entered: Option<&Entered>) -> Result<(), String> {
        self.config.validate()?;
        if let Some(entered) = entered {
            let login = match (entered.user.as_str(), entered.password.as_str()) {
                // Empty fields mean "no login" only if we know what we would remove.
                ("", "") if self.stored_login.is_some() => None,
                ("", _) | (_, "") => {
                    return Err(self
                        .t(
                            "Benutzername und Passwort ausfüllen (oder beide leeren).",
                            "Enter both user name and password (or clear both).",
                        )
                        .to_owned());
                }
                (user, password) => Some((user.to_owned(), password.to_owned())),
            };
            if self.stored_login.as_ref() != Some(&login) {
                login::save(login.as_ref().map(|(u, p)| (u.as_str(), p.as_str())))?;
                self.stored_login = Some(login);
            }
        }
        self.config.save()?;
        // The app has to reload even if switching the autostart fails afterwards.
        self.exit = SAVED;
        if let Some(entered) = entered
            && entered.autostart != self.autostart_before
        {
            if entered.autostart {
                autostart::enable()?;
                self.exit = INSTALLED;
            } else {
                autostart::disable()?;
            }
            self.autostart_before = entered.autostart;
        }
        Ok(())
    }
}

/// Buttons and error line at the bottom of both windows.
struct Footer {
    error: HWND,
    defaults: Option<HWND>,
    cancel: HWND,
    save: HWND,
}

impl Footer {
    fn new(window: &Window, app: &App, defaults: bool) -> Self {
        let button = |text: &str, id, look| {
            window.button(window.handle, text, [0, 0, 120, 34], id, look, BACKGROUND)
        };
        let defaults = defaults.then(|| {
            button(
                app.t("Standardwerte", "Defaults"),
                DEFAULTS,
                Look::Secondary,
            )
        });
        let cancel = button(app.t("Abbrechen", "Cancel"), CANCEL, Look::Secondary);
        let save = button(app.t("Speichern", "Save"), SAVE, Look::Primary);
        let error = window.label(
            window.handle,
            "",
            [20, 0, 100, 36],
            Font::Small,
            ERROR,
            BACKGROUND,
        );
        Self {
            error,
            defaults,
            cancel,
            save,
        }
    }

    fn layout(&self, window: &Window) {
        let (width, height) = window.size();
        let y = height - FOOTER + 16;
        window.move_to(self.save, [width - 140, y, 120, 34]);
        window.move_to(self.cancel, [width - 270, y, 120, 34]);
        let mut left = width - 270;
        if let Some(defaults) = self.defaults {
            left -= 140;
            window.move_to(defaults, [left, y, 130, 34]);
        }
        window.move_to(self.error, [20, y - 2, left - 30, 40]);
    }
}

/// Shows the window until it is closed. Returns the exit code for the process.
pub fn run(page: Page) -> Result<i32, Box<dyn std::error::Error>> {
    // SAFETY: plain call before any window exists.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    match page {
        Page::Settings => settings(),
        Page::Sensors => sensors(),
        Page::Setup => install(),
        Page::Uninstall => uninstall(),
    }
}

/// A card with a title and rows of "label on the left, control on the right".
struct Card<'a> {
    window: &'a Window,
    panel: HWND,
    /// Top of the next row.
    y: i32,
}

impl Card<'_> {
    const LEFT: i32 = 20;
    const WIDTH: i32 = 480;
    const RIGHT: i32 = Self::LEFT + Self::WIDTH - 16;

    fn new<'a>(window: &'a Window, panel: HWND, y: i32, rows: i32, title: &str) -> Card<'a> {
        window.card(
            panel,
            [Self::LEFT, y, Self::WIDTH, 38 + rows * 36 + 6],
            CARD,
        );
        window.label(
            panel,
            title,
            [36, y + 12, 200, 18],
            Font::Small,
            MUTED,
            CARD,
        );
        Card {
            window,
            panel,
            y: y + 38,
        }
    }

    /// Adds the label and returns the area for a control of this width.
    fn row(&mut self, label: &str, width: i32) -> [i32; 4] {
        self.window.label(
            self.panel,
            label,
            [36, self.y + 7, 220, 20],
            Font::Normal,
            TEXT,
            CARD,
        );
        let area = [Self::RIGHT - width, self.y + 2, width, 30];
        self.y += 36;
        area
    }

    fn switch(&mut self, label: &str, on: bool) -> HWND {
        let [x, y, width, _] = self.row(label, 44);
        let control = self.window.button(
            self.panel,
            label,
            [x, y + 3, width, 24],
            0,
            Look::Switch,
            CARD,
        );
        ui::set_checked(control, on);
        control
    }

    fn edit(&mut self, label: &str, text: &str, width: i32, style: i32, limit: usize) -> HWND {
        let area = self.row(label, width);
        self.window.edit(self.panel, text, area, style, limit)
    }

    fn combo(&mut self, label: &str, items: &[&str], at: Option<usize>, width: i32) -> HWND {
        let [x, y, width, _] = self.row(label, width);
        self.window
            .combo(self.panel, items, at, [x, y + 2, width, 28])
    }

    /// A row whose control is replaced by an explanation.
    fn note(&mut self, label: &str, note: &str) {
        let [x, y, width, _] = self.row(label, 270);
        self.window.label(
            self.panel,
            note,
            [x, y + 5, width, 20],
            Font::Small,
            MUTED,
            CARD,
        );
    }

    /// A line across the card, for hints.
    fn hint(&mut self, text: &str, color: u32) -> HWND {
        let control = self.window.label(
            self.panel,
            text,
            [36, self.y + 4, Self::WIDTH - 32, 20],
            Font::Small,
            color,
            CARD,
        );
        self.y += 28;
        control
    }
}

fn settings() -> Result<i32, Box<dyn std::error::Error>> {
    let (mut app, mut problem) = App::load();
    let mut user = String::new();
    let mut password = String::new();
    match login::saved() {
        Ok(stored) => {
            if let Some((u, p)) = &stored {
                user.clone_from(u);
                password.clone_from(p);
            }
            app.stored_login = Some(stored);
        }
        Err(e) => problem = Some(e),
    }
    let mut ports: Vec<String> = serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default();
    let config = app.config.clone();
    if !config.serial_port.is_empty() && !ports.contains(&config.serial_port) {
        ports.push(config.serial_port.clone());
    }
    app.autostart_before = autostart::enabled();
    let old_autostart = autostart::old_app_enabled();
    let from_environment = std::env::var_os("PULSEMQTT_USER").is_some();
    let language = config.language;
    let t = move |german: &'static str, english: &'static str| language.pick(german, english);

    let window = Window::new(
        t("PulseMQTT - Einstellungen", "PulseMQTT - Settings"),
        540,
        900,
    )?;
    let panel = window.panel([0, 0, 540, 900 - FOOTER], BACKGROUND);
    window.label(
        panel,
        "PulseMQTT",
        [20, 14, 400, 34],
        Font::Large,
        TEXT,
        BACKGROUND,
    );
    window.label(
        panel,
        t("Einstellungen", "Settings"),
        [20, 48, 400, 20],
        Font::Normal,
        MUTED,
        BACKGROUND,
    );

    // The two hints share the place of a sixth row.
    let mut card = Card::new(&window, panel, 80, 6, "MQTT");
    let mqtt = card.switch(t("MQTT senden", "Send via MQTT"), config.mqtt_enabled);
    let port = card.edit("Port", &config.port.to_string(), 90, ES_NUMBER, 5);
    let topic = card.edit("Topic", &config.topic, 230, 0, 200);
    let user = card.edit(t("Benutzername", "User name"), &user, 230, 0, 100);
    let password = card.edit(t("Passwort", "Password"), &password, 230, ES_PASSWORD, 200);
    let hint_y = card.y;
    let environment_hint = card.hint(
        t(
            "Die Umgebungsvariablen PULSEMQTT_* haben Vorrang.",
            "The PULSEMQTT_* environment variables take precedence.",
        ),
        MUTED,
    );
    card.y = hint_y;
    let open_hint = card.hint(
        t(
            "Ohne Login kann sich jeder im Netzwerk verbinden.",
            "Without a login everyone on the network can connect.",
        ),
        ERROR,
    );

    let mut y = card.y + 24;
    let mut card = Card::new(&window, panel, y, 4, t("SERIELL / USB", "SERIAL / USB"));
    let serial = card.switch(
        t("USB/Seriell senden", "Send via USB/serial"),
        config.serial_enabled,
    );
    let names: Vec<&str> = ports.iter().map(String::as_str).collect();
    let at = ports.iter().position(|p| *p == config.serial_port);
    let serial_port = card.combo(t("Serieller Port", "Serial port"), &names, at, 110);
    let at = MODES.iter().position(|m| *m == config.serial_mode);
    let mode = card.combo(
        t("Verbindungsart", "Connection type"),
        &["USB-Serial/JTAG (ESP32-C3)", "UART0 (USB-Seriell-Chip)"],
        at,
        240,
    );
    let interval = card.edit(
        t("Update-Intervall (s)", "Update interval (s)"),
        &format!("{:.1}", config.interval_seconds),
        90,
        0,
        4,
    );

    y = card.y + 18;
    let mut card = Card::new(&window, panel, y, 4, t("ALLGEMEIN", "GENERAL"));
    let autostart = if autostart::possible() {
        Some(card.switch(
            t("Mit Windows starten", "Start with Windows"),
            app.autostart_before,
        ))
    } else {
        card.note(
            t("Mit Windows starten", "Start with Windows"),
            t(
                "als Administrator neu starten (Tray-Menü)",
                "restart as administrator (tray menu)",
            ),
        );
        None
    };
    let auto_update = if update::configured() {
        Some(card.switch(
            t("Automatische Updates", "Automatic updates"),
            config.auto_update,
        ))
    } else {
        card.note(
            t("Automatische Updates", "Automatic updates"),
            t("noch nicht verfügbar", "not available yet"),
        );
        None
    };
    let at = LANGUAGES.iter().position(|l| *l == config.language);
    let language_box = card.combo(
        t("Sprache", "Language"),
        &[t("Automatisch", "Automatic"), "Deutsch", "English"],
        at,
        140,
    );
    let old_hint = card.hint(
        t(
            "Die alte PulseMQTT-App startet auch noch mit Windows (gleicher Port).",
            "The old PulseMQTT app also still starts with Windows (same port).",
        ),
        ERROR,
    );

    y = card.y + 18;
    window.label(
        panel,
        t(
            "Topic muss mit der Firmware-Konfiguration übereinstimmen. Über USB werden \
             dieselben JSON-Daten als Textzeilen (115200 Baud) gesendet.",
            "The topic must match the firmware configuration. USB sends the same JSON data \
             as text lines (115200 baud).",
        ),
        [20, y, Card::WIDTH, 40],
        Font::Small,
        MUTED,
        BACKGROUND,
    );
    let content = y + 52;

    let footer = Footer::new(&window, &app, false);
    if let Some(problem) = &problem {
        ui::set_text(footer.error, problem);
    }
    let hints = |window_state: &App| {
        let empty = ui::text(user).is_empty() && ui::text(password).is_empty();
        ui::show(environment_hint, from_environment);
        ui::show(open_hint, !from_environment && empty);
        let starts = autostart.map_or(window_state.autostart_before, ui::checked);
        ui::show(old_hint, starts && old_autostart);
    };
    let layout = || {
        let (width, height) = window.size();
        window.move_to(panel, [0, 0, width, height - FOOTER]);
        footer.layout(&window);
    };
    layout();
    window.set_content_height(panel, content);
    hints(&app);

    window.run(panel, None, |event| {
        match event {
            Event::Command(SAVE, BN_CLICKED) => {
                let config = &mut app.config;
                config.mqtt_enabled = ui::checked(mqtt);
                // Invalid input becomes 0, which validation rejects with a message.
                config.port = ui::text(port).parse().unwrap_or(0);
                config.topic = ui::text(topic);
                config.serial_enabled = ui::checked(serial);
                config.serial_port = ui::selected(serial_port)
                    .and_then(|i| ports.get(i).cloned())
                    .unwrap_or_default();
                config.serial_mode = ui::selected(mode).map_or(config.serial_mode, |i| MODES[i]);
                config.interval_seconds =
                    ui::text(interval).replace(',', ".").parse().unwrap_or(0.0);
                if let Some(control) = auto_update {
                    config.auto_update = ui::checked(control);
                }
                config.language =
                    ui::selected(language_box).map_or(config.language, |i| LANGUAGES[i]);
                let entered = Entered {
                    user: ui::text(user),
                    password: ui::text(password),
                    autostart: autostart.map_or(app.autostart_before, ui::checked),
                };
                match app.save(Some(&entered)) {
                    Ok(()) => return false,
                    Err(e) => ui::set_text(footer.error, &e),
                }
            }
            Event::Command(CANCEL, BN_CLICKED) | Event::Close => return false,
            Event::Command(ui::RESIZED, _) => layout(),
            Event::Command(0, BN_CLICKED | EN_CHANGE) => hints(&app),
            _ => {}
        }
        true
    });
    Ok(app.exit)
}

/// The window of the setup file.
fn install() -> Result<i32, Box<dyn std::error::Error>> {
    let language = App::load().0.config.language;
    let t = move |german: &'static str, english: &'static str| language.pick(german, english);
    let installed = autostart::program().exists();
    let this = env!("CARGO_PKG_VERSION");

    let window = Window::new(t("PulseMQTT - Installation", "PulseMQTT - Setup"), 540, 330)?;
    let main = window.handle;
    let panel = window.panel([0, 0, 540, 330 - FOOTER], BACKGROUND);
    let label =
        |text: &str, area, font, color| window.label(panel, text, area, font, color, BACKGROUND);
    label("PulseMQTT", [20, 14, 400, 34], Font::Large, TEXT);
    let state = match setup::installed_version() {
        Some(version) => format!(
            "{} {version}",
            t("Installiert ist Version", "Installed: version")
        ),
        None if installed => t("Bereits installiert", "Already installed").to_owned(),
        None => t("Noch nicht installiert", "Not installed yet").to_owned(),
    };
    label(&state, [20, 48, 500, 20], Font::Normal, MUTED);

    let mut card = Card::new(&window, panel, 80, 1, t("INSTALLATION", "INSTALLATION"));
    // An existing installation keeps its choice; a new one starts with Windows.
    let on = !installed || autostart::enabled();
    let starts = card.switch(t("Mit Windows starten", "Start with Windows"), on);
    label(
        t(
            "Installiert nach C:\\Program Files\\PulseMQTT-Rust. Windows fragt dafür nach \
             Administratorrechten.",
            "Installs to C:\\Program Files\\PulseMQTT-Rust. Windows asks for administrator \
             rights.",
        ),
        [20, card.y + 20, Card::WIDTH, 40],
        Font::Small,
        MUTED,
    );

    let button =
        |text: &str, id, look| window.button(main, text, [0, 0, 10, 10], id, look, BACKGROUND);
    let confirm = if installed {
        format!("{this} {}", t("installieren", "install"))
    } else {
        t("Installieren", "Install").to_owned()
    };
    let confirm = button(&confirm, SAVE, Look::Primary);
    let cancel = button(t("Abbrechen", "Cancel"), CANCEL, Look::Secondary);
    let remove =
        installed.then(|| button(t("Deinstallieren", "Uninstall"), UNINSTALL, Look::Secondary));
    let error = window.label(main, "", [0, 0, 10, 10], Font::Small, ERROR, BACKGROUND);
    let layout = || {
        let (width, height) = window.size();
        let y = height - FOOTER + 16;
        window.move_to(panel, [0, 0, width, height - FOOTER - 24]);
        window.move_to(error, [20, y - 38, width - 40, 36]);
        window.move_to(confirm, [width - 160, y, 140, 34]);
        window.move_to(cancel, [width - 290, y, 120, 34]);
        if let Some(remove) = remove {
            window.move_to(remove, [20, y, 130, 34]);
        }
    };
    layout();

    window.run(panel, None, |event| {
        let arguments: Vec<String> = match event {
            Event::Command(SAVE, BN_CLICKED) => {
                let mut arguments = vec!["--install".to_owned()];
                if ui::checked(starts) {
                    arguments.push("--enable-autostart".to_owned());
                }
                arguments
            }
            Event::Command(UNINSTALL, BN_CLICKED) => vec!["--uninstall".to_owned()],
            Event::Command(CANCEL, BN_CLICKED) | Event::Close => return false,
            Event::Command(ui::RESIZED, _) => {
                layout();
                return true;
            }
            _ => return true,
        };
        match setup::elevate(&arguments) {
            Ok(()) => false,
            Err(e) => {
                ui::set_text(error, &e);
                true
            }
        }
    });
    Ok(0)
}

/// Started from Settings > Apps or the setup window, with admin rights.
fn uninstall() -> Result<i32, Box<dyn std::error::Error>> {
    let language = App::load().0.config.language;
    let t = move |german: &'static str, english: &'static str| language.pick(german, english);

    let window = Window::new(
        t("PulseMQTT - Deinstallation", "PulseMQTT - Uninstall"),
        540,
        260,
    )?;
    let main = window.handle;
    let panel = window.panel([0, 0, 540, 260 - FOOTER], BACKGROUND);
    window.label(
        panel,
        t("PulseMQTT deinstallieren?", "Uninstall PulseMQTT?"),
        [20, 14, 500, 34],
        Font::Large,
        TEXT,
        BACKGROUND,
    );
    let mut card = Card::new(&window, panel, 64, 1, t("DATEN", "DATA"));
    let purge = card.switch(
        t(
            "Auch Einstellungen und Login löschen",
            "Also delete settings and login",
        ),
        false,
    );

    let button =
        |text: &str, id, look| window.button(main, text, [0, 0, 10, 10], id, look, BACKGROUND);
    let confirm = button(t("Deinstallieren", "Uninstall"), UNINSTALL, Look::Primary);
    let cancel = button(t("Abbrechen", "Cancel"), CANCEL, Look::Secondary);
    let error = window.label(main, "", [0, 0, 10, 10], Font::Small, ERROR, BACKGROUND);
    let layout = || {
        let (width, height) = window.size();
        let y = height - FOOTER + 16;
        window.move_to(panel, [0, 0, width, height - FOOTER - 24]);
        window.move_to(error, [20, y - 38, width - 40, 36]);
        window.move_to(confirm, [width - 160, y, 140, 34]);
        window.move_to(cancel, [width - 290, y, 120, 34]);
    };
    layout();

    let mut removed = false;
    window.run(panel, None, |event| {
        match event {
            Event::Command(UNINSTALL, BN_CLICKED) => match setup::uninstall(ui::checked(purge)) {
                Ok(()) => {
                    removed = true;
                    return false;
                }
                Err(e) => ui::set_text(error, &e),
            },
            Event::Command(CANCEL, BN_CLICKED) | Event::Close => return false,
            Event::Command(ui::RESIZED, _) => layout(),
            _ => {}
        }
        true
    });
    if removed {
        tray::show_info(t("PulseMQTT wurde entfernt.", "PulseMQTT was removed."));
        // Last, so that the program is no longer in use when it is deleted.
        setup::delete_self_later();
    }
    Ok(0)
}

/// The controls of one sensor in the table.
struct Row {
    id: String,
    group: Group,
    check: HWND,
    texts: [HWND; 3],
    key: HWND,
    value: HWND,
}

impl Row {
    /// Places the row at `y` of the table, or hides it.
    fn place(&self, window: &Window, panel: HWND, y: i32, visible: bool) {
        window.move_to(self.check, [16, y + 7, 20, 20]);
        for (control, (x, width)) in self.texts.iter().zip([(52, 90), (150, 105), (265, 175)]) {
            window.move_to(*control, [x, y + 7, width, 20]);
            ui::show(*control, visible);
        }
        window.place_field(panel, self.key, [450, y + 2, 220, 30], visible);
        window.move_to(self.value, [690, y + 7, 120, 20]);
        ui::show(self.check, visible);
        ui::show(self.value, visible);
    }
}

fn value_text(reading: &Reading) -> String {
    match reading.value {
        Some(v) => format!("{v:.1} {}", reading.unit()),
        None => "n/a".to_owned(),
    }
}

fn sensors() -> Result<i32, Box<dyn std::error::Error>> {
    let (mut app, problem) = App::load();
    let mut source = Sensors::open();
    let readings = source.read();
    let language = app.config.language;
    let t = move |german: &'static str, english: &'static str| language.pick(german, english);

    let window = Window::new(t("PulseMQTT - Sensoren", "PulseMQTT - Sensors"), 900, 680)?;
    let main = window.handle;
    let label =
        |text: &str, area, font, color| window.label(main, text, area, font, color, BACKGROUND);
    label(
        t("Sensoren konfigurieren", "Configure sensors"),
        [20, 14, 600, 34],
        Font::Large,
        TEXT,
    );
    label(
        t(
            "Wähle die Sensoren, die veröffentlicht werden",
            "Select the sensors to publish",
        ),
        [20, 48, 600, 20],
        Font::Normal,
        MUTED,
    );
    let summary = |x: i32, title: &str| {
        window.card(main, [x, 80, 420, 74], CARD);
        window.label(main, title, [x + 16, 90, 200, 18], Font::Small, MUTED, CARD);
        window.label(main, "", [x + 16, 110, 180, 36], Font::Large, TEXT, CARD)
    };
    let count = summary(20, t("AUSGEWÄHLT", "SELECTED"));
    // The ESP32 firmware keeps at most this many values.
    let too_many = window.label(
        main,
        t("Das Display nutzt nur 24.", "The display uses only 24."),
        [220, 120, 200, 18],
        Font::Small,
        ERROR,
        CARD,
    );
    let refresh = summary(460, t("AKTUALISIERUNG", "UPDATE"));
    let interval = app.config.interval_seconds.clamp(0.5, 60.0);
    ui::set_text(refresh, &format!("{interval:.1} s"));

    let names = [
        t("Alle", "All"),
        "CPU",
        "GPU",
        t("Speicher", "Memory"),
        t("Laufwerke", "Drives"),
    ];
    let filters: Vec<HWND> = (0u16..)
        .zip(names)
        .map(|(i, name)| {
            let area = [20 + i32::from(i) * 108, 166, 100, 30];
            window.button(main, name, area, FILTER + i, Look::Chip, BACKGROUND)
        })
        .collect();
    ui::set_checked(filters[0], true);
    label(
        t("Alle erkannten anzeigen", "Show all detected"),
        [640, 171, 180, 20],
        Font::Normal,
        TEXT,
    );
    // Created after its label, which names it for screen readers.
    let show_all = window.button(
        main,
        t("Alle erkannten anzeigen", "Show all detected"),
        [836, 169, 44, 24],
        SHOW_ALL,
        Look::Switch,
        BACKGROUND,
    );
    for (title, x) in [
        ("HARDWARE", 72),
        (t("TYP", "TYPE"), 170),
        ("SENSOR", 285),
        (t("MQTT-SCHLÜSSEL", "MQTT KEY"), 470),
        (t("AKTUELL", "CURRENT"), 710),
    ] {
        label(title, [x, TABLE - 24, 150, 18], Font::Small, MUTED);
    }

    let panel = window.panel([20, TABLE, 860, 300], CARD);
    let mut ordered: Vec<&Reading> = readings.iter().collect();
    ordered.sort_by_key(|r| FILTERS.iter().position(|f| *f == Some(r.group)));
    let rows: Vec<Row> = ordered
        .into_iter()
        .map(|reading| {
            let sensor = app.config.sensor(&reading.id);
            let name = name(&reading.id, language);
            let text = |text: &str, color| {
                window.label(panel, text, [0, 0, 10, 10], Font::Normal, color, CARD)
            };
            let check = window.button(panel, &name, [0, 0, 20, 20], 0, Look::CheckBox, CARD);
            ui::set_checked(check, sensor.enabled);
            let hardware = text(hardware(reading.group, language), MUTED);
            let kind = text(kind(reading.kind, language), MUTED);
            // Right before the field, so that it names the field for screen readers.
            let title = text(&name, TEXT);
            let key = window.edit(panel, &sensor.key, [0, 0, 220, 30], 0, MAX_KEY);
            let value = text(&value_text(reading), TEXT);
            Row {
                id: reading.id.clone(),
                group: reading.group,
                check,
                texts: [hardware, kind, title],
                key,
                value,
            }
        })
        .collect();

    let footer = Footer::new(&window, &app, true);
    if let Some(problem) = &problem {
        ui::set_text(footer.error, problem);
    }
    let layout = || {
        let (width, height) = window.size();
        window.move_to(panel, [20, TABLE, width - 40, height - TABLE - FOOTER]);
        footer.layout(&window);
    };
    let arrange = || {
        let filter = filters
            .iter()
            .position(|f| ui::checked(*f))
            .and_then(|i| FILTERS[i]);
        let all = ui::checked(show_all);
        // Selected extra sensors stay visible when the others are hidden.
        let shown = |row: &Row| {
            filter.is_none_or(|group| group == row.group)
                && (all || !config::extra(&row.id) || ui::checked(row.check))
        };
        let visible: Vec<bool> = rows.iter().map(shown).collect();
        let height = 8 + 36 * visible.iter().filter(|v| **v).count() as i32;
        // Back to the top before the rows get their new places.
        window.set_content_height(panel, height);
        let mut y = 4;
        for (row, visible) in rows.iter().zip(visible) {
            row.place(&window, panel, y, visible);
            if visible {
                y += 36;
            }
        }
        // The field frames have to be painted at their new places.
        ui::repaint(panel);
    };
    let counted = || {
        let selected = rows.iter().filter(|r| ui::checked(r.check)).count();
        ui::set_text(count, &format!("{selected} {}", t("Sensoren", "sensors")));
        ui::show(too_many, selected > 24);
    };
    layout();
    arrange();
    counted();

    let milliseconds = (interval * 1000.0) as u32;
    window.run(panel, Some(milliseconds), |event| {
        match event {
            Event::Timer => {
                for reading in source.read() {
                    if let Some(row) = rows.iter().find(|r| r.id == reading.id) {
                        ui::set_text(row.value, &value_text(&reading));
                    }
                }
            }
            Event::Command(SAVE, BN_CLICKED) => {
                for row in &rows {
                    let sensor = Sensor {
                        enabled: ui::checked(row.check),
                        key: ui::text(row.key),
                    };
                    if sensor != app.config.sensor(&row.id) {
                        app.config.sensors.insert(row.id.clone(), sensor);
                    }
                }
                match app.save(None) {
                    Ok(()) => return false,
                    Err(e) => ui::set_text(footer.error, &e),
                }
            }
            Event::Command(CANCEL, BN_CLICKED) | Event::Close => return false,
            Event::Command(DEFAULTS, BN_CLICKED) => {
                app.config.sensors.clear();
                for row in &rows {
                    let sensor = app.config.sensor(&row.id);
                    ui::set_checked(row.check, sensor.enabled);
                    ui::set_text(row.key, &sensor.key);
                }
                counted();
            }
            Event::Command(id, BN_CLICKED)
                if id == SHOW_ALL || (FILTER..FILTER + 5).contains(&id) =>
            {
                arrange();
            }
            Event::Command(ui::RESIZED, _) => {
                layout();
                ui::repaint(main);
            }
            Event::Command(0, BN_CLICKED) => counted(),
            _ => {}
        }
        true
    });
    Ok(app.exit)
}
fn hardware(group: Group, language: Language) -> &'static str {
    match group {
        Group::Cpu => "CPU",
        Group::Gpu => "GPU",
        Group::Memory => language.pick("Speicher", "Memory"),
        Group::Drive => language.pick("Laufwerk", "Drive"),
    }
}

fn kind(kind: Kind, language: Language) -> &'static str {
    let (german, english) = match kind {
        Kind::Load => ("Last", "Load"),
        Kind::Clock => ("Takt", "Clock"),
        Kind::Power => ("Leistung", "Power"),
        Kind::Temperature => ("Temperatur", "Temperature"),
        Kind::Fan => ("Lüfter", "Fan"),
        Kind::Data => ("Daten", "Data"),
        Kind::Voltage => ("Spannung", "Voltage"),
    };
    language.pick(german, english)
}

/// Display name of a sensor id.
fn name(id: &str, language: Language) -> String {
    let t = |german: &'static str, english: &'static str| language.pick(german, english);
    if let Some(drive) = id.strip_prefix("storage_temp_") {
        return format!("{} {drive}", t("Temperatur", "Temperature"));
    }
    if let Some(header) = id.strip_prefix("fan_header_") {
        return format!("{} {header}", t("Lüfteranschluss", "Fan header"));
    }
    if let Some(number) = id.strip_prefix("cpu_load_") {
        return format!("{} {number}", t("Prozessor", "Processor"));
    }
    if let Some(letter) = id.strip_prefix("disk_usage_") {
        let letter = letter.to_uppercase();
        return format!("{} {letter}:", t("Belegung", "Usage"));
    }
    match id {
        "cpu_load" | "gpu_load" => t("Auslastung", "Load"),
        "cpu_power" | "gpu_power" => t("Leistungsaufnahme", "Power draw"),
        "cpu_temp" | "gpu_temp" => t("Temperatur", "Temperature"),
        "cpu_fan" => t("CPU-Lüfter", "CPU fan"),
        "gpu_core_clock" => t("Kerntakt", "Core clock"),
        "gpu_fan" => t("Lüfterdrehzahl", "Fan speed"),
        "gpu_fan_percent" => t("Lüfter in Prozent", "Fan percent"),
        "memory_available" => t("Verfügbar", "Available"),
        "memory_used" => t("Belegt", "Used"),
        "memory_total" => t("Gesamt", "Total"),
        "gpu_hotspot_temp" => t("Hotspot-Temperatur", "Hotspot temperature"),
        "gpu_memory_clock" => t("Speichertakt", "Memory clock"),
        "gpu_memory_used" => t("Grafikspeicher belegt", "Memory used"),
        "gpu_voltage" => t("Spannung", "Voltage"),
        "gpu_board_power" => t("Leistung ganze Karte", "Board power"),
        other => other,
    }
    .to_owned()
}
