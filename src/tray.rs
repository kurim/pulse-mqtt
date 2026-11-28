//! Notification area icon with an owner-drawn menu in the Windows 11 style
//! (light or dark, following the system setting).

use crate::{
    config::Language,
    gui,
    ui::{rgb, wide},
    update,
};
use std::{
    cell::RefCell,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    ptr::{null, null_mut},
    sync::mpsc::{Receiver, Sender, channel},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{Dwm::DwmSetWindowAttribute, Gdi::*},
    System::Threading::CREATE_NO_WINDOW,
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    },
    UI::{
        Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED},
        HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI},
        Shell::*,
        WindowsAndMessaging::*,
    },
};

const WM_TRAY: u32 = WM_APP + 1;
/// Posted when a settings window has saved changes.
const WM_SAVED: u32 = WM_APP + 2;
/// Posted when a new version has been installed.
const WM_UPDATED: u32 = WM_APP + 3;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const ICON_ID: u32 = 1;
const SEPARATOR: usize = 0;
const SETTINGS: usize = 1;
const SENSORS: usize = 2;
const RESTART: usize = 3;
const PAWNIO: usize = 4;
const UPDATES: usize = 5;
const QUIT: usize = 6;

#[derive(Clone)]
struct Entry {
    id: usize,
    /// Glyph from the Segoe icon fonts.
    icon: char,
    text: String,
    enabled: bool,
}

fn entry(id: usize, icon: char, text: &str, enabled: bool) -> Entry {
    Entry {
        id,
        icon,
        text: text.to_owned(),
        enabled,
    }
}

pub enum Event {
    Quit,
    /// Settings changed: the app has to start again.
    Reload,
    /// Autostart was set up: continue with the copy in Program Files.
    Switch,
}

/// What the menu depends on.
struct Available {
    windows: bool,
    restart: bool,
    updates: bool,
}

fn entries(language: Language, available: Available, pawnio: bool) -> Vec<Entry> {
    let t = |german, english| language.pick(german, english);
    let separator = || entry(SEPARATOR, ' ', "", false);
    let (pawnio_icon, pawnio_text) = if pawnio {
        ('\u{E73E}', t("PawnIO installiert", "PawnIO installed"))
    } else {
        (
            '\u{E7BA}',
            t("PawnIO nicht installiert", "PawnIO not installed"),
        )
    };
    let updates = format!(
        "{} (v{})",
        t("Nach Updates suchen...", "Check for updates..."),
        env!("CARGO_PKG_VERSION")
    );
    vec![
        entry(
            SETTINGS,
            '\u{E713}',
            t("Einstellungen...", "Settings..."),
            available.windows,
        ),
        entry(
            SENSORS,
            '\u{E9D9}',
            t("Sensoren auswählen...", "Select sensors..."),
            available.windows,
        ),
        separator(),
        entry(
            RESTART,
            '\u{EA18}',
            t("Als Administrator neu starten", "Restart as administrator"),
            available.restart,
        ),
        entry(PAWNIO, pawnio_icon, pawnio_text, false),
        separator(),
        entry(UPDATES, '\u{E895}', &updates, available.updates),
        separator(),
        entry(QUIT, '\u{E7E8}', t("Beenden", "Exit"), true),
    ]
}

struct State {
    events: Sender<Event>,
    language: Language,
    /// Only one settings window at a time.
    window_open: bool,
    tooltip: String,
    taskbar_created: u32,
    menu: Vec<Entry>,
    dpi: u32,
    dark: bool,
}

thread_local! {
    // Only the tray thread uses this. Never keep it borrowed across a call that pumps messages.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

const CLASS: &str = "PulseMqttTray";

/// Whether a tray icon of this app exists, in any process.
pub fn running() -> bool {
    // SAFETY: a NUL-terminated class name.
    unsafe { !FindWindowW(wide(CLASS).as_ptr(), null()).is_null() }
}

/// Shows the icon on its own thread and reports what the user did.
pub fn spawn(tooltip: String, language: Language, auto_update: bool) -> Receiver<Event> {
    let (events, receiver) = channel();
    thread::spawn(move || run(events, tooltip, language, auto_update));
    receiver
}

fn run(events: Sender<Event>, tooltip: String, language: Language, auto_update: bool) {
    // SAFETY: plain Win32 window setup; all strings are NUL-terminated and outlive the calls.
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide(CLASS);
        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                events,
                language,
                window_open: false,
                tooltip,
                taskbar_created,
                menu: Vec::new(),
                dpi: 96,
                dark: true,
            })
        });
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&window_class);
        // A hidden top-level window: message-only windows miss the TaskbarCreated broadcast.
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            eprintln!("Tray icon unavailable: could not create its window");
            return;
        }
        // Explorer is not elevated; let its restart notice through when we are.
        ChangeWindowMessageFilterEx(window, taskbar_created, MSGFLT_ALLOW, null_mut());
        add_icon(window);
        if auto_update && update::configured() {
            update_automatically(window);
        }

        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data for which all zeros is valid.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = ICON_ID;
    data
}

unsafe fn add_icon(window: HWND) {
    let mut data = icon_data(window);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = WM_TRAY;
    if let Some(tooltip) = with_state(|s| s.tooltip.clone()) {
        // Zip stops one short, so the terminating NUL stays.
        let room = data.szTip.len() - 1;
        for (to, from) in data.szTip[..room].iter_mut().zip(tooltip.encode_utf16()) {
            *to = from;
        }
    }
    // SAFETY: resource 1 is the embedded icon; `data` is fully initialised.
    unsafe {
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(window);
        data.hIcon = LoadImageW(
            GetModuleHandleW(null()),
            std::ptr::without_provenance(1),
            IMAGE_ICON,
            GetSystemMetricsForDpi(SM_CXSMICON, dpi),
            GetSystemMetricsForDpi(SM_CYSMICON, dpi),
            LR_SHARED,
        );
        Shell_NotifyIconW(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the pointers in lparam are the structures Windows documents for these messages.
    unsafe {
        match message {
            WM_TRAY => {
                // Version 4: event in the low word of lparam, screen position in wparam.
                // WM_CONTEXTMENU covers both right click and the keyboard.
                if u32::from(lparam as u16) == WM_CONTEXTMENU {
                    let x = i32::from(wparam as u16 as i16);
                    let y = i32::from((wparam >> 16) as u16 as i16);
                    show_menu(window, POINT { x, y });
                }
                0
            }
            WM_MEASUREITEM => {
                measure(&mut *(lparam as *mut MEASUREITEMSTRUCT));
                1
            }
            WM_DRAWITEM => {
                draw(&*(lparam as *const DRAWITEMSTRUCT));
                1
            }
            // Sent while the menu is open; lparam is the menu's own window.
            WM_ENTERIDLE if wparam == MSGF_MENU as usize => {
                round_corners(lparam as HWND);
                0
            }
            // A settings window closed; wparam is its exit code.
            WM_SAVED => {
                with_state(|s| s.window_open = false);
                match wparam as i32 {
                    gui::SAVED => finish(window, Event::Reload),
                    gui::INSTALLED => finish(window, Event::Switch),
                    _ => {}
                }
                0
            }
            WM_UPDATED => {
                finish(window, Event::Reload);
                0
            }
            _ if Some(message) == with_state(|s| s.taskbar_created) => {
                add_icon(window);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn show_menu(window: HWND, at: POINT) {
    // The user may have restarted as administrator without the environment variables,
    // which would drop the login. Do not offer the restart in that case.
    // SAFETY: no arguments.
    let elevated = unsafe { IsUserAnAdmin() } != 0;
    let can_restart = !elevated && std::env::var_os("PULSEMQTT_USER").is_none();
    let (language, window_open) = with_state(|s| (s.language, s.window_open)).unwrap_or_default();
    let available = Available {
        windows: !window_open,
        restart: can_restart,
        updates: update::configured(),
    };
    let menu_entries = entries(language, available, crate::pawnio::installed());
    let dark = !uses_light_theme();

    // SAFETY: the menu and brush live until after TrackPopupMenuEx returns.
    let chosen = unsafe {
        let mut dpi = (96, 96);
        let monitor = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi.0, &mut dpi.1);
        with_state(|s| {
            s.menu = menu_entries.clone();
            s.dpi = dpi.0;
            s.dark = dark;
        });

        let menu = CreatePopupMenu();
        let background = CreateSolidBrush(Palette::new(dark).background);
        let info = MENUINFO {
            cbSize: size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND,
            hbrBack: background,
            ..std::mem::zeroed()
        };
        SetMenuInfo(menu, &info);
        for (index, e) in menu_entries.iter().enumerate() {
            // The string is not drawn from here, but screen readers use it.
            let mut text = wide(&e.text);
            let mut item: MENUITEMINFOW = std::mem::zeroed();
            item.cbSize = size_of::<MENUITEMINFOW>() as u32;
            item.fMask = MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_DATA | MIIM_STRING;
            item.fType = MFT_OWNERDRAW;
            if e.id == SEPARATOR {
                item.fType |= MFT_SEPARATOR;
            }
            item.fState = if e.enabled { MFS_ENABLED } else { MFS_DISABLED };
            item.wID = e.id as u32;
            item.dwItemData = index;
            item.dwTypeData = text.as_mut_ptr();
            InsertMenuItemW(menu, index as u32, 1, &item);
        }
        // Both calls are needed for the menu to close when the user clicks elsewhere.
        SetForegroundWindow(window);
        let chosen = TrackPopupMenuEx(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            at.x,
            at.y,
            window,
            null(),
        );
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
        DeleteObject(background);
        chosen as usize
    };

    // SAFETY: `window` is our live tray window.
    unsafe {
        match chosen {
            SETTINGS => open_window(window, "--settings"),
            SENSORS => open_window(window, "--sensors"),
            UPDATES => update_now(window, language),
            RESTART if restart_elevated() => finish(window, Event::Quit),
            QUIT => finish(window, Event::Quit),
            _ => {}
        }
    }
}

/// Runs a settings window as a child process and reports back when it closes.
fn open_window(window: HWND, page: &str) {
    let child = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg(page)
            .creation_flags(CREATE_NO_WINDOW)
            // Without a console (autostart) there are no handles that could be inherited.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    });
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return eprintln!("Could not open the window: {e}"),
    };
    with_state(|s| s.window_open = true);
    let window = window as usize;
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        // SAFETY: posting to a window handle is allowed from any thread.
        unsafe { PostMessageW(window as HWND, WM_SAVED, code.unwrap_or(0) as WPARAM, 0) };
    });
}

fn post(window: usize, message: u32) {
    // SAFETY: posting to a window handle is allowed from any thread.
    unsafe { PostMessageW(window as HWND, message, 0, 0) };
}

/// Checks at start and then once a day.
fn update_automatically(window: HWND) {
    let window = window as usize;
    thread::spawn(move || {
        loop {
            let installed = update::check().and_then(|found| match found {
                Some(release) => update::install(&release).map(|()| true),
                None => Ok(false),
            });
            match installed {
                Ok(true) => return post(window, WM_UPDATED),
                Ok(false) => {}
                Err(e) => eprintln!("Update failed: {e}"),
            }
            thread::sleep(DAY);
        }
    });
}

/// The menu entry: reports the result and asks before installing.
fn update_now(window: HWND, language: Language) {
    let window = window as usize;
    thread::spawn(move || {
        let t = |german, english| language.pick(german, english);
        let result = update::check().and_then(|found| {
            let Some(release) = found else {
                message(
                    t(
                        "Du hast die neueste Version.",
                        "You have the latest version.",
                    ),
                    MB_OK,
                );
                return Ok(false);
            };
            let question = format!(
                "{} {}. {}",
                t("Verfügbar ist Version", "Available: version"),
                release.version,
                t("Jetzt installieren?", "Install it now?")
            );
            if message(&question, MB_YESNO) != IDYES {
                return Ok(false);
            }
            update::install(&release).map(|()| true)
        });
        match result {
            Ok(true) => post(window, WM_UPDATED),
            Ok(false) => {}
            Err(e) => {
                let failed = t("Update fehlgeschlagen", "Update failed");
                message(&format!("{failed}: {e}"), MB_OK | MB_ICONWARNING);
            }
        }
    });
}

pub fn show_error(text: &str) {
    message(text, MB_OK | MB_ICONERROR);
}

pub fn show_info(text: &str) {
    message(text, MB_OK | MB_ICONINFORMATION);
}

fn message(text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    // SAFETY: NUL-terminated strings that outlive the call.
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide("PulseMQTT").as_ptr(),
            style | MB_SETFOREGROUND,
        )
    }
}

unsafe fn finish(window: HWND, event: Event) {
    // SAFETY: removes our own icon.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &icon_data(window));
        with_state(|s| s.events.send(event).ok());
        PostQuitMessage(0);
    }
}

/// Starts an elevated copy; false if the user declines the UAC prompt.
fn restart_elevated() -> bool {
    // The new process waits for this one so that the broker port is free again.
    let mut arguments = vec![format!("--after={}", std::process::id())];
    arguments
        .extend(std::env::args().filter(|a| ["--background", "--console"].contains(&a.as_str())));
    crate::setup::elevate(&arguments).is_ok()
}

fn uses_light_theme() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` has the size passed in.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),
            wide("SystemUsesLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    status == 0 && value != 0
}

struct Palette {
    background: u32,
    hover: u32,
    text: u32,
    disabled: u32,
    separator: u32,
}

impl Palette {
    const fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: rgb(44, 44, 44),
                hover: rgb(61, 61, 61),
                text: rgb(255, 255, 255),
                disabled: rgb(120, 120, 120),
                separator: rgb(64, 64, 64),
            }
        } else {
            Self {
                background: rgb(249, 249, 249),
                hover: rgb(234, 234, 234),
                text: rgb(26, 26, 26),
                disabled: rgb(160, 160, 160),
                separator: rgb(226, 226, 226),
            }
        }
    }
}

fn scale(pixels: i32, dpi: u32) -> i32 {
    pixels * dpi as i32 / 96
}

unsafe fn font(face: &str, pixels: i32) -> HFONT {
    // SAFETY: the face name is NUL-terminated.
    unsafe {
        CreateFontW(
            -pixels,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            CLIP_DEFAULT_PRECIS.into(),
            CLEARTYPE_QUALITY.into(),
            DEFAULT_PITCH.into(),
            wide(face).as_ptr(),
        )
    }
}

unsafe fn measure(item: &mut MEASUREITEMSTRUCT) {
    let Some(Some((e, dpi))) =
        with_state(|s| s.menu.get(item.itemData).cloned().map(|e| (e, s.dpi)))
    else {
        return;
    };
    if e.id == SEPARATOR {
        item.itemHeight = scale(7, dpi) as u32;
        return;
    }
    let text: Vec<u16> = e.text.encode_utf16().collect();
    let mut size = SIZE { cx: 0, cy: 0 };
    // SAFETY: a screen DC and a temporary font, both released below.
    unsafe {
        let dc = GetDC(null_mut());
        let font = font("Segoe UI", scale(12, dpi));
        let previous = SelectObject(dc, font);
        GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
        SelectObject(dc, previous);
        DeleteObject(font);
        ReleaseDC(null_mut(), dc);
    }
    item.itemHeight = scale(28, dpi) as u32;
    item.itemWidth = (size.cx + scale(34 + 12, dpi)) as u32;
}

unsafe fn draw(item: &DRAWITEMSTRUCT) {
    let Some(Some((e, dpi, dark))) = with_state(|s| {
        let e = s.menu.get(item.itemData).cloned();
        e.map(|e| (e, s.dpi, s.dark))
    }) else {
        return;
    };
    let colors = Palette::new(dark);
    let area = item.rcItem;
    // SAFETY: draws into the DC Windows provides and restores or frees every object.
    unsafe {
        let dc = item.hDC;
        fill(dc, &area, colors.background);
        if e.id == SEPARATOR {
            let middle = (area.top + area.bottom) / 2;
            let line = RECT {
                top: middle,
                bottom: middle + scale(1, dpi).max(1),
                ..area
            };
            fill(dc, &line, colors.separator);
            return;
        }
        if e.enabled && item.itemState & ODS_SELECTED != 0 {
            let brush = CreateSolidBrush(colors.hover);
            let old_brush = SelectObject(dc, brush);
            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            let (inset, radius) = (scale(4, dpi), scale(8, dpi));
            RoundRect(
                dc,
                area.left + inset,
                area.top + 1,
                area.right - inset,
                area.bottom,
                radius,
                radius,
            );
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            DeleteObject(brush);
        }
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(
            dc,
            if e.enabled {
                colors.text
            } else {
                colors.disabled
            },
        );
        // Windows 11 ships "Segoe Fluent Icons"; Windows 10 only the MDL2 predecessor.
        let icons = if std::path::Path::new(r"C:\Windows\Fonts\SegoeIcons.ttf").exists() {
            "Segoe Fluent Icons"
        } else {
            "Segoe MDL2 Assets"
        };
        let mut glyph = [0; 2];
        text_at(
            dc,
            icons,
            scale(14, dpi),
            e.icon.encode_utf16(&mut glyph),
            RECT {
                left: area.left + scale(11, dpi),
                ..area
            },
        );
        let text: Vec<u16> = e.text.encode_utf16().collect();
        text_at(
            dc,
            "Segoe UI",
            scale(12, dpi),
            &text,
            RECT {
                left: area.left + scale(34, dpi),
                ..area
            },
        );
    }
}

unsafe fn fill(dc: HDC, area: &RECT, color: u32) {
    // SAFETY: temporary brush, deleted right away.
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, area, brush);
        DeleteObject(brush);
    }
}

/// Left-aligned, vertically centred single line.
unsafe fn text_at(dc: HDC, face: &str, pixels: i32, text: &[u16], mut area: RECT) {
    // SAFETY: the length is passed explicitly; the font is restored and deleted.
    unsafe {
        let font = font(face, pixels);
        let previous = SelectObject(dc, font);
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut area,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        SelectObject(dc, previous);
        DeleteObject(font);
    }
}

unsafe fn round_corners(menu_window: HWND) {
    const CORNER_PREFERENCE: u32 = 33; // DWMWA_WINDOW_CORNER_PREFERENCE
    const BORDER_COLOR: u32 = 34; // DWMWA_BORDER_COLOR
    let round = 2u32; // DWMWCP_ROUND
    let border = Palette::new(with_state(|s| s.dark).unwrap_or(true)).separator;
    // SAFETY: both values are u32 with matching size. Fails harmlessly before Windows 11.
    unsafe {
        for (attribute, value) in [(CORNER_PREFERENCE, &round), (BORDER_COLOR, &border)] {
            DwmSetWindowAttribute(menu_window, attribute, std::ptr::from_ref(value).cast(), 4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_matches_the_state() {
        let enabled = |list: Vec<Entry>| -> Vec<usize> {
            list.iter().filter(|e| e.enabled).map(|e| e.id).collect()
        };
        let available = |on| Available {
            windows: on,
            restart: on,
            updates: on,
        };
        let menu = |on| entries(Language::De, available(on), false);
        assert_eq!(
            enabled(menu(true)),
            [SETTINGS, SENSORS, RESTART, UPDATES, QUIT]
        );
        assert_eq!(enabled(menu(false)), [QUIT]);
        assert!(
            menu(true)
                .iter()
                .any(|e| e.text == "PawnIO nicht installiert")
        );
        assert!(
            entries(Language::En, available(true), true)
                .iter()
                .any(|e| e.text == "Exit")
        );
    }
}
