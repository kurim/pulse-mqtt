//! The settings and sensor windows (egui), run as their own process.

use crate::{
    autostart,
    config::{Config, Language, MAX_KEY, Sensor, SerialMode},
    login,
    sensors::{Group, Kind, Reading, Sensors},
    update,
};
use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
        WindowsAndMessaging::{
            ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON,
            SendMessageW, WM_SETICON,
        },
    },
};

/// Exit code of a window process that saved changes.
pub const SAVED: i32 = 10;
/// Saved, and the app was just set up to start with Windows: continue with that copy.
pub const INSTALLED: i32 = 11;

const BACKGROUND: Color32 = Color32::from_rgb(13, 14, 18);
const CARD: Color32 = Color32::from_rgb(22, 25, 33);
const FIELD: Color32 = Color32::from_rgb(13, 15, 21);
const BORDER: Color32 = Color32::from_rgb(38, 43, 53);
const ACCENT: Color32 = Color32::from_rgb(74, 222, 128);
const TEXT: Color32 = Color32::from_rgb(230, 233, 238);
const MUTED: Color32 = Color32::from_rgb(125, 133, 148);
const ERROR: Color32 = Color32::from_rgb(255, 110, 110);

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Settings,
    Sensors,
}

impl Page {
    pub fn from_argument(argument: &str) -> Option<Self> {
        match argument {
            "--settings" => Some(Self::Settings),
            "--sensors" => Some(Self::Sensors),
            _ => None,
        }
    }
}

/// Shows the window until it is closed. Returns the exit code for the process.
pub fn run(page: Page) -> Result<i32, Box<dyn std::error::Error>> {
    let saved = Arc::new(AtomicI32::new(0));
    let app = App::new(page, saved.clone())?;
    let title = match page {
        Page::Settings => app.t("PulseMQTT - Einstellungen", "PulseMQTT - Settings"),
        Page::Sensors => app.t("PulseMQTT - Sensoren", "PulseMQTT - Sensors"),
    }
    .to_owned();
    let size = match page {
        Page::Settings => [540.0, 940.0],
        Page::Sensors => [960.0, 680.0],
    };
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_min_inner_size([480.0, 400.0])
        // Empty: keeps egui from setting its own icon, see `set_icon`.
        .with_icon(egui::IconData::default());
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(|creation| {
            style(&creation.egui_ctx);
            if let Ok(handle) = creation.window_handle()
                && let RawWindowHandle::Win32(window) = handle.as_raw()
            {
                let window = window.hwnd.get() as HWND;
                match_title_bar(window);
                set_icon(window);
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(saved.load(Ordering::Relaxed))
}

/// Gives the title bar the colours of the window (Windows 11; ignored before).
fn match_title_bar(window: HWND) {
    let color = |c: Color32| u32::from_le_bytes([c.r(), c.g(), c.b(), 0]);
    let attributes = [
        (DWMWA_CAPTION_COLOR, color(BACKGROUND)),
        (DWMWA_BORDER_COLOR, color(BORDER)),
        (DWMWA_TEXT_COLOR, color(TEXT)),
    ];
    for (attribute, value) in attributes {
        // SAFETY: the value is a u32 (COLORREF) with matching size.
        unsafe {
            DwmSetWindowAttribute(window, attribute as u32, (&raw const value).cast(), 4);
        }
    }
}

/// Uses the icon embedded in the program instead of carrying a second copy of it.
fn set_icon(window: HWND) {
    let sizes = [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)];
    for (which, metric) in sizes {
        // SAFETY: resource 1 is the embedded icon; LR_SHARED icons need no cleanup.
        unsafe {
            let side = GetSystemMetricsForDpi(metric, GetDpiForWindow(window));
            let icon = LoadImageW(
                GetModuleHandleW(std::ptr::null()),
                std::ptr::without_provenance(1),
                IMAGE_ICON,
                side,
                side,
                LR_SHARED,
            );
            SendMessageW(window, WM_SETICON, which as usize, icon as isize);
        }
    }
}

/// Segoe UI from Windows, so that no fonts have to be built into the program.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::empty();
    // The second font only fills in symbols that the first one lacks.
    for file in ["segoeui.ttf", "seguisym.ttf"] {
        let Ok(data) = std::fs::read(format!(r"C:\Windows\Fonts\{file}")) else {
            continue;
        };
        let data = Arc::new(egui::FontData::from_owned(data));
        fonts.font_data.insert(file.to_owned(), data);
        for family in fonts.families.values_mut() {
            family.push(file.to_owned());
        }
    }
    fonts
}

fn style(context: &egui::Context) {
    context.set_fonts(fonts());
    context.set_theme(egui::Theme::Dark);
    context.global_style_mut(|style| {
        let v = &mut style.visuals;
        v.panel_fill = BACKGROUND;
        v.window_fill = CARD;
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = Color32::from_rgb(27, 31, 40);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = Color32::from_rgb(31, 94, 60);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(8);
        }
        v.widgets.inactive.weak_bg_fill = CARD;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        // Check marks.
        v.widgets.inactive.fg_stroke = Stroke::new(2.0, ACCENT);
        style.spacing.button_padding = egui::vec2(16.0, 8.0);
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        for font in style.text_styles.values_mut() {
            font.size *= 1.1;
        }
    });
}

struct App {
    page: Page,
    config: Config,
    /// Problem shown above the buttons.
    error: Option<String>,
    saved: Arc<AtomicI32>,

    // Settings page
    user: String,
    password: String,
    /// What is stored; `None` if it could not be read.
    stored_login: Option<Option<(String, String)>>,
    ports: Vec<String>,
    autostart: bool,
    autostart_before: bool,
    old_autostart: bool,

    // Sensor page
    sensors: Option<Sensors>,
    readings: Vec<Reading>,
    next_read: Instant,
    filter: Option<Group>,
}

impl App {
    fn new(page: Page, saved: Arc<AtomicI32>) -> Result<Self, sysinfo::Error> {
        // A broken file is reported; saving then replaces it with what the window shows.
        let (config, mut error) = match Config::load() {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(e)),
        };
        let mut app = Self {
            page,
            config,
            error: None,
            saved,
            user: String::new(),
            password: String::new(),
            stored_login: None,
            ports: Vec::new(),
            autostart: false,
            autostart_before: false,
            old_autostart: false,
            sensors: None,
            readings: Vec::new(),
            next_read: Instant::now(),
            filter: None,
        };
        match page {
            Page::Settings => {
                match login::saved() {
                    Ok(stored) => {
                        if let Some((user, password)) = &stored {
                            app.user.clone_from(user);
                            app.password.clone_from(password);
                        }
                        app.stored_login = Some(stored);
                    }
                    Err(e) => error = Some(e),
                }
                app.ports = serialport::available_ports()
                    .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
                    .unwrap_or_default();
                app.autostart = autostart::enabled();
                app.autostart_before = app.autostart;
                app.old_autostart = autostart::old_app_enabled();
            }
            Page::Sensors => app.sensors = Some(Sensors::open()?),
        }
        app.error = error;
        Ok(app)
    }

    fn t<'a>(&self, german: &'a str, english: &'a str) -> &'a str {
        self.config.language.pick(german, english)
    }

    fn save(&mut self) -> Result<(), String> {
        self.config.validate()?;
        if self.page == Page::Settings {
            let login = match (self.user.as_str(), self.password.as_str()) {
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
        self.saved.store(SAVED, Ordering::Relaxed);
        if self.autostart != self.autostart_before {
            if self.autostart {
                autostart::enable()?;
                self.saved.store(INSTALLED, Ordering::Relaxed);
            } else {
                autostart::disable()?;
            }
            self.autostart_before = self.autostart;
        }
        Ok(())
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        let save = egui::Button::new(
            RichText::new(self.t("Speichern", "Save"))
                .color(BACKGROUND)
                .strong(),
        )
        .fill(ACCENT);
        // Right-to-left layouts place the first widget at the far right.
        if ui.add(save).clicked() {
            match self.save() {
                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.error = Some(e),
            }
        }
        if ui.button(self.t("Abbrechen", "Cancel")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.page == Page::Sensors && ui.button(self.t("Standardwerte", "Defaults")).clicked() {
            self.config.sensors.clear();
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let language = self.config.language;
        let t = move |german: &'static str, english: &'static str| language.pick(german, english);
        heading(ui, "PulseMQTT", t("Einstellungen", "Settings"));

        card(ui, "MQTT", |ui| {
            row(ui, t("MQTT senden", "Send via MQTT"), |ui, label| {
                switch(ui, &mut self.config.mqtt_enabled, label);
            });
            row(ui, "Port", |ui, _| {
                ui.add(egui::DragValue::new(&mut self.config.port).range(1..=u16::MAX));
            });
            row(ui, "Topic", |ui, _| {
                field(ui, &mut self.config.topic, false);
            });
            row(ui, t("Benutzername", "User name"), |ui, _| {
                field(ui, &mut self.user, false);
            });
            row(ui, t("Passwort", "Password"), |ui, _| {
                field(ui, &mut self.password, true);
            });
            if std::env::var_os("PULSEMQTT_USER").is_some() {
                ui.colored_label(
                    MUTED,
                    t(
                        "Die Umgebungsvariablen PULSEMQTT_* haben Vorrang.",
                        "The PULSEMQTT_* environment variables take precedence.",
                    ),
                );
            } else if self.user.is_empty() && self.password.is_empty() {
                ui.colored_label(
                    ERROR,
                    t(
                        "Ohne Login kann sich jeder im Netzwerk verbinden.",
                        "Without a login everyone on the network can connect.",
                    ),
                );
            }
        });

        card(ui, t("SERIELL / USB", "SERIAL / USB"), |ui| {
            row(
                ui,
                t("USB/Seriell senden", "Send via USB/serial"),
                |ui, label| {
                    switch(ui, &mut self.config.serial_enabled, label);
                },
            );
            row(ui, t("Serieller Port", "Serial port"), |ui, _| {
                egui::ComboBox::from_id_salt("serial_port")
                    .selected_text(match self.config.serial_port.as_str() {
                        "" => "-",
                        port => port,
                    })
                    .show_ui(ui, |ui| {
                        for port in &self.ports {
                            ui.selectable_value(&mut self.config.serial_port, port.clone(), port);
                        }
                    });
            });
            row(ui, t("Verbindungsart", "Connection type"), |ui, _| {
                let name = |mode| match mode {
                    SerialMode::UsbSerialJtag => "USB-Serial/JTAG (ESP32-C3)",
                    SerialMode::Uart0 => "UART0 (USB-Seriell-Chip)",
                };
                egui::ComboBox::from_id_salt("serial_mode")
                    .selected_text(name(self.config.serial_mode))
                    .show_ui(ui, |ui| {
                        for mode in [SerialMode::UsbSerialJtag, SerialMode::Uart0] {
                            ui.selectable_value(&mut self.config.serial_mode, mode, name(mode));
                        }
                    });
            });
            row(ui, t("Update-Intervall", "Update interval"), |ui, _| {
                ui.add(
                    egui::DragValue::new(&mut self.config.interval_seconds)
                        .range(0.5..=60.0)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .suffix(" s"),
                );
            });
        });

        card(ui, t("ALLGEMEIN", "GENERAL"), |ui| {
            row(
                ui,
                t("Mit Windows starten", "Start with Windows"),
                |ui, label| {
                    if autostart::possible() {
                        switch(ui, &mut self.autostart, label);
                    } else {
                        ui.colored_label(
                            MUTED,
                            t(
                                "dafür als Administrator neu starten (Tray-Menü)",
                                "restart as administrator first (tray menu)",
                            ),
                        );
                    }
                },
            );
            if self.autostart && self.old_autostart {
                ui.colored_label(
                    ERROR,
                    t(
                        "Die alte PulseMQTT-App startet auch noch mit Windows (gleicher Port).",
                        "The old PulseMQTT app also still starts with Windows (same port).",
                    ),
                );
            }
            row(
                ui,
                t("Automatische Updates", "Automatic updates"),
                |ui, label| {
                    if update::configured() {
                        switch(ui, &mut self.config.auto_update, label);
                    } else {
                        ui.colored_label(MUTED, t("noch nicht verfügbar", "not available yet"));
                    }
                },
            );
            row(ui, t("Sprache", "Language"), |ui, _| {
                let name = |l| match l {
                    Language::Auto => t("Automatisch", "Automatic"),
                    Language::De => "Deutsch",
                    Language::En => "English",
                };
                egui::ComboBox::from_id_salt("language")
                    .selected_text(name(self.config.language))
                    .show_ui(ui, |ui| {
                        for l in [Language::Auto, Language::De, Language::En] {
                            ui.selectable_value(&mut self.config.language, l, name(l));
                        }
                    });
            });
        });

        ui.label(
            RichText::new(t(
                "Topic muss mit der Firmware-Konfiguration übereinstimmen. Über USB werden \
                 dieselben JSON-Daten als Textzeilen (115200 Baud) gesendet.",
                "The topic must match the firmware configuration. USB sends the same JSON \
                 data as text lines (115200 baud).",
            ))
            .color(MUTED)
            .small(),
        );
    }

    fn sensor_page(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let interval = Duration::from_secs_f64(self.config.interval_seconds.clamp(0.5, 60.0));
        if now >= self.next_read
            && let Some(sensors) = &mut self.sensors
        {
            self.readings = sensors.read();
            self.next_read = now + interval;
        }
        ui.ctx().request_repaint_after(interval);

        let language = self.config.language;
        let t = move |german: &'static str, english: &'static str| language.pick(german, english);
        heading(
            ui,
            t("Sensoren konfigurieren", "Configure sensors"),
            t(
                "Wähle die Sensoren, die veröffentlicht werden",
                "Select the sensors to publish",
            ),
        );
        let selected = self
            .readings
            .iter()
            .filter(|r| self.config.sensor(&r.id).enabled)
            .count();
        ui.columns(2, |columns| {
            card(&mut columns[0], t("AUSGEWÄHLT", "SELECTED"), |ui| {
                ui.label(
                    RichText::new(format!("{selected} {}", t("Sensoren", "sensors"))).size(26.0),
                );
                // The ESP32 firmware keeps at most this many values.
                if selected > 24 {
                    ui.colored_label(
                        ERROR,
                        t("Das Display nutzt nur 24.", "The display uses only 24."),
                    );
                }
            });
            card(&mut columns[1], t("AKTUALISIERUNG", "UPDATE"), |ui| {
                ui.label(RichText::new(format!("{:.1} s", interval.as_secs_f64())).size(26.0));
            });
        });
        ui.horizontal(|ui| {
            let filters = [
                (None, t("Alle", "All")),
                (Some(Group::Cpu), "CPU"),
                (Some(Group::Gpu), "GPU"),
                (Some(Group::Memory), t("Speicher", "Memory")),
                (Some(Group::Drive), t("Laufwerke", "Drives")),
            ];
            for (group, name) in filters {
                ui.selectable_value(&mut self.filter, group, name);
            }
        });

        // Only the table scrolls; the summary and the filters stay in place.
        card(ui, "", |ui| {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                egui::Grid::new("sensors")
                    .num_columns(6)
                    .striped(true)
                    .spacing([24.0, 12.0])
                    .show(ui, |ui| {
                        for title in [
                            "",
                            "HARDWARE",
                            t("TYP", "TYPE"),
                            "SENSOR",
                            t("MQTT-SCHLÜSSEL", "MQTT KEY"),
                            t("AKTUELL", "CURRENT"),
                        ] {
                            ui.colored_label(MUTED, title);
                        }
                        ui.end_row();
                        for r in &self.readings {
                            if self.filter.is_some_and(|g| g != r.group) {
                                continue;
                            }
                            let before = self.config.sensor(&r.id);
                            let mut sensor: Sensor = before.clone();
                            let name = name(&r.id, language);
                            ui.checkbox(&mut sensor.enabled, "").widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::CheckBox,
                                    true,
                                    before.enabled,
                                    &name,
                                )
                            });
                            ui.colored_label(MUTED, hardware(r.group, language));
                            ui.colored_label(MUTED, kind(r.kind, language));
                            ui.label(&name);
                            ui.add_sized(
                                [220.0, 30.0],
                                egui::TextEdit::singleline(&mut sensor.key).char_limit(MAX_KEY),
                            );
                            ui.label(match r.value {
                                Some(v) => format!("{v:.1} {}", r.unit()),
                                None => "n/a".to_owned(),
                            });
                            ui.end_row();
                            if sensor != before {
                                self.config.sensors.insert(r.id.clone(), sensor);
                            }
                        }
                    });
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let margin = Frame::new().fill(BACKGROUND).inner_margin(Margin::same(20));
        egui::Panel::bottom("buttons").frame(margin).show(ui, |ui| {
            if let Some(error) = &self.error {
                ui.colored_label(ERROR, error);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.buttons(ui));
        });
        egui::CentralPanel::default()
            .frame(margin)
            .show(ui, |ui| match self.page {
                Page::Settings => {
                    egui::ScrollArea::both()
                        .auto_shrink(false)
                        .show(ui, |ui| self.settings(ui));
                }
                Page::Sensors => self.sensor_page(ui),
            });
    }
}

fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(24.0).strong());
    ui.colored_label(MUTED, subtitle);
    ui.add_space(8.0);
}

fn card(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).color(MUTED).small().strong());
            }
            content(ui);
        });
    ui.add_space(4.0);
}

/// Label on the left, control on the right. The label id names the control for screen readers.
fn row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui, &str)) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        let id = ui.label(label).id;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.scope(|ui| control(ui, label)).response.labelled_by(id);
        });
    });
}

fn field(ui: &mut egui::Ui, text: &mut String, password: bool) {
    ui.add(
        egui::TextEdit::singleline(text)
            .password(password)
            .desired_width(220.0)
            .margin(Margin::symmetric(10, 8)),
    );
}

/// An on/off switch.
fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) {
    let (area, mut response) = ui.allocate_exact_size(egui::vec2(42.0, 24.0), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *on, label)
    });
    let position = ui.ctx().animate_bool(response.id, *on);
    let painter = ui.painter();
    let fill = if *on { ACCENT } else { BORDER };
    painter.rect_filled(area, 12.0, fill);
    if response.has_focus() {
        painter.rect_stroke(
            area,
            12.0,
            Stroke::new(1.5, TEXT),
            egui::StrokeKind::Outside,
        );
    }
    let x = egui::lerp(area.left() + 12.0..=area.right() - 12.0, position);
    let knob = if *on { BACKGROUND } else { MUTED };
    painter.circle_filled(egui::pos2(x, area.center().y), 8.0, knob);
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
        other => other,
    }
    .to_owned()
}
