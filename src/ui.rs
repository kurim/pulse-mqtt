//! Small toolkit for the app's own windows: native Windows controls (so that keyboard
//! and screen readers work as usual) painted with GDI in a dark style. No graphics driver
//! is loaded, which keeps the windows small.
//!
//! All positions are given in 96 dpi pixels.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        },
        Gdi::*,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::*,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem},
        Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_RETURN},
        WindowsAndMessaging::*,
    },
};

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

pub const BACKGROUND: u32 = rgb(13, 14, 18);
pub const CARD: u32 = rgb(22, 25, 33);
pub const FIELD: u32 = rgb(13, 15, 21);
pub const BORDER: u32 = rgb(38, 43, 53);
pub const ACCENT: u32 = rgb(74, 222, 128);
pub const ACCENT_DARK: u32 = rgb(31, 94, 60);
pub const HOVER: u32 = rgb(33, 38, 49);
pub const TEXT: u32 = rgb(230, 233, 238);
pub const MUTED: u32 = rgb(125, 133, 148);
pub const ERROR: u32 = rgb(255, 110, 110);

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Font {
    Normal,
    Small,
    Large,
}

#[derive(Clone, Copy)]
pub enum Look {
    /// The main action, filled with the accent colour.
    Primary,
    Secondary,
    /// A filter; highlighted while selected.
    Chip,
    Switch,
    CheckBox,
}

#[derive(Clone, Copy)]
struct Style {
    text: u32,
    /// What is behind the control.
    background: u32,
    look: Look,
}

#[derive(Clone, Copy)]
struct Shape {
    area: RECT,
    fill: u32,
    /// The control this frames, so that both can be moved together.
    owner: usize,
    visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A button was pressed or a value changed: control id and notification code.
    Command(u16, u32),
    Timer,
    Close,
}

#[derive(Default)]
struct State {
    styles: HashMap<usize, Style>,
    /// Rounded rectangles painted behind the controls of a surface, in device pixels.
    boxes: HashMap<usize, Vec<Shape>>,
    brushes: HashMap<u32, usize>,
    /// Scroll position and content height of scrolling panels, in device pixels.
    scroll: HashMap<usize, (i32, i32)>,
    events: VecDeque<Event>,
    buttons: HashSet<usize>,
}

thread_local! {
    // The windows live on one thread. Never keep it borrowed across a call that sends messages.
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with_borrow_mut(f)
}

fn brush(color: u32) -> HBRUSH {
    state(|s| {
        *s.brushes.entry(color).or_insert_with(|| {
            // SAFETY: brushes are kept until the process ends.
            unsafe { CreateSolidBrush(color) as usize }
        }) as HBRUSH
    })
}

/// A window with its controls. The process ends when it is closed.
#[derive(Clone, Copy)]
pub struct Window {
    pub handle: HWND,
    dpi: u32,
    fonts: [HFONT; 3],
}

impl Window {
    /// `width` and `height` are the size of the content.
    pub fn new(title: &str, width: i32, height: i32) -> Result<Self, String> {
        // shortcut: laid out once for the system dpi; handle WM_DPICHANGED if moving the
        // window between differently scaled monitors should resize its content.
        // SAFETY: plain window setup; strings are NUL-terminated and outlive the calls.
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES,
            };
            InitCommonControlsEx(&controls);
            let dpi = GetDpiForSystem();
            let instance = GetModuleHandleW(null());
            for (name, procedure) in [
                (
                    "PulseMqttWindow",
                    window_proc as unsafe extern "system" fn(_, _, _, _) -> _,
                ),
                ("PulseMqttPanel", panel_proc),
            ] {
                let name = wide(name);
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance,
                    // Resource 1 is the embedded app icon.
                    hIcon: LoadIconW(instance, std::ptr::without_provenance(1)),
                    hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                    lpszClassName: name.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&class);
            }
            let scale = |v: i32| v * dpi as i32 / 96;
            let style = WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX | WS_CLIPCHILDREN;
            let mut area = RECT {
                left: 0,
                top: 0,
                right: scale(width),
                bottom: scale(height),
            };
            AdjustWindowRectExForDpi(&mut area, style, 0, 0, dpi);
            let mut screen: RECT = std::mem::zeroed();
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut screen).cast(), 0);
            let w = (area.right - area.left).min(screen.right - screen.left);
            let h = (area.bottom - area.top).min(screen.bottom - screen.top);
            let handle = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                wide("PulseMqttWindow").as_ptr(),
                wide(title).as_ptr(),
                style,
                screen.left + (screen.right - screen.left - w) / 2,
                screen.top + (screen.bottom - screen.top - h) / 2,
                w,
                h,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if handle.is_null() {
                return Err("could not create the window".into());
            }
            for (attribute, value) in [
                (DWMWA_USE_IMMERSIVE_DARK_MODE, 1),
                (DWMWA_CAPTION_COLOR, BACKGROUND),
                (DWMWA_BORDER_COLOR, BORDER),
                (DWMWA_TEXT_COLOR, TEXT),
            ] {
                // Windows 11 colours; ignored before.
                DwmSetWindowAttribute(handle, attribute as u32, (&raw const value).cast(), 4);
            }
            let font = |points: i32, weight: u32| {
                CreateFontW(
                    -scale(points),
                    0,
                    0,
                    0,
                    weight as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET.into(),
                    OUT_DEFAULT_PRECIS.into(),
                    CLIP_DEFAULT_PRECIS.into(),
                    CLEARTYPE_QUALITY.into(),
                    DEFAULT_PITCH.into(),
                    wide("Segoe UI").as_ptr(),
                )
            };
            let fonts = [
                font(14, FW_NORMAL),
                font(12, FW_NORMAL),
                font(24, FW_SEMIBOLD),
            ];
            Ok(Self { handle, dpi, fonts })
        }
    }

    pub fn scale(&self, value: i32) -> i32 {
        value * self.dpi as i32 / 96
    }

    fn rect(&self, [x, y, w, h]: [i32; 4]) -> RECT {
        RECT {
            left: self.scale(x),
            top: self.scale(y),
            right: self.scale(x + w),
            bottom: self.scale(y + h),
        }
    }

    /// The client area in 96 dpi pixels.
    pub fn size(&self) -> (i32, i32) {
        // SAFETY: the handle is our window.
        let area = unsafe {
            let mut area: RECT = std::mem::zeroed();
            GetClientRect(self.handle, &mut area);
            area
        };
        let back = |v: i32| v * 96 / self.dpi as i32;
        (back(area.right), back(area.bottom))
    }

    // The arguments are those of CreateWindowExW.
    #[allow(clippy::too_many_arguments)]
    fn control(
        &self,
        parent: HWND,
        class: &str,
        text: &str,
        style: u32,
        area: RECT,
        id: u16,
        font: Font,
    ) -> HWND {
        // SAFETY: strings are NUL-terminated; the parent is one of our windows.
        unsafe {
            let control = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                parent,
                std::ptr::without_provenance_mut(id.into()),
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(control, WM_SETFONT, self.fonts[font as usize] as usize, 0);
            control
        }
    }

    fn remember(control: HWND, text: u32, background: u32, look: Look) {
        let style = Style {
            text,
            background,
            look,
        };
        state(|s| s.styles.insert(control as usize, style));
    }

    /// A line of text. Also names the control created right after it for screen readers.
    pub fn label(
        &self,
        parent: HWND,
        text: &str,
        area: [i32; 4],
        font: Font,
        color: u32,
        background: u32,
    ) -> HWND {
        // SS_NOPREFIX: show "&" as it is.
        const NO_PREFIX: u32 = 0x80;
        let control = self.control(parent, "STATIC", text, NO_PREFIX, self.rect(area), 0, font);
        Self::remember(control, color, background, Look::Secondary);
        control
    }

    /// A rounded rectangle behind the controls.
    pub fn card(&self, parent: HWND, area: [i32; 4], fill: u32) {
        self.shape(parent, area, fill, 0);
    }

    fn shape(&self, parent: HWND, area: [i32; 4], fill: u32, owner: usize) {
        let shape = Shape {
            area: self.rect(area),
            fill,
            owner,
            visible: true,
        };
        state(|s| s.boxes.entry(parent as usize).or_default().push(shape));
    }

    /// Moves or hides a text field together with its frame.
    pub fn place_field(&self, parent: HWND, control: HWND, area: [i32; 4], visible: bool) {
        let [x, y, w, h] = area;
        let outer = self.rect(area);
        state(|s| {
            for shape in s.boxes.entry(parent as usize).or_default() {
                if shape.owner == control as usize {
                    shape.area = outer;
                    shape.visible = visible;
                }
            }
        });
        self.move_to(control, [x + 8, y + (h - 20) / 2, w - 16, 20]);
        show(control, visible);
    }

    /// A text field inside a rounded frame.
    pub fn edit(&self, parent: HWND, text: &str, area: [i32; 4], extra: i32, limit: usize) -> HWND {
        let [x, y, w, h] = area;
        let inner = self.rect([x + 8, y + (h - 20) / 2, w - 16, 20]);
        let style = WS_TABSTOP | ES_AUTOHSCROLL as u32 | extra as u32;
        let control = self.control(parent, "EDIT", text, style, inner, 0, Font::Normal);
        Self::remember(control, TEXT, FIELD, Look::Secondary);
        self.shape(parent, area, FIELD, control as usize);
        // SAFETY: a plain value message to our control.
        unsafe { SendMessageW(control, EM_SETLIMITTEXT, limit, 0) };
        control
    }

    pub fn button(
        &self,
        parent: HWND,
        text: &str,
        area: [i32; 4],
        id: u16,
        look: Look,
        background: u32,
    ) -> HWND {
        let kind = match look {
            Look::Primary | Look::Secondary => BS_PUSHBUTTON,
            Look::Chip => BS_AUTORADIOBUTTON | BS_PUSHLIKE,
            Look::Switch | Look::CheckBox => BS_AUTOCHECKBOX,
        };
        let style = WS_TABSTOP | kind as u32;
        let control = self.control(
            parent,
            "BUTTON",
            text,
            style,
            self.rect(area),
            id,
            Font::Normal,
        );
        Self::remember(control, TEXT, background, look);
        state(|s| s.buttons.insert(control as usize));
        control
    }

    /// A drop-down list.
    pub fn combo(
        &self,
        parent: HWND,
        items: &[&str],
        selected: Option<usize>,
        area: [i32; 4],
    ) -> HWND {
        let [x, y, w, _] = area;
        // The height includes the opened list.
        let area = self.rect([x, y, w, 240]);
        let style = WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32;
        let control = self.control(parent, "COMBOBOX", "", style, area, 0, Font::Normal);
        Self::remember(control, TEXT, FIELD, Look::Secondary);
        // SAFETY: the strings outlive the synchronous messages.
        unsafe {
            SetWindowTheme(control, wide("DarkMode_CFD").as_ptr(), null());
            for item in items {
                SendMessageW(control, CB_ADDSTRING, 0, wide(item).as_ptr() as LPARAM);
            }
            if let Some(index) = selected {
                SendMessageW(control, CB_SETCURSEL, index, 0);
            }
        }
        control
    }

    /// A vertically scrolling area for controls.
    pub fn panel(&self, area: [i32; 4], background: u32) -> HWND {
        let area = self.rect(area);
        // SAFETY: our own window class and parent.
        unsafe {
            let panel = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                wide("PulseMqttPanel").as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                self.handle,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            SetWindowTheme(panel, wide("DarkMode_Explorer").as_ptr(), null());
            Self::remember(panel, TEXT, background, Look::Secondary);
            panel
        }
    }

    pub fn move_to(&self, control: HWND, area: [i32; 4]) {
        let area = self.rect(area);
        // SAFETY: our own control.
        unsafe {
            MoveWindow(
                control,
                area.left,
                area.top,
                area.right - area.left,
                area.bottom - area.top,
                1,
            )
        };
    }

    /// Sets the height of a panel's content and returns to the top.
    pub fn set_content_height(&self, panel: HWND, height: i32) {
        let height = self.scale(height);
        scroll_to(panel, 0);
        state(|s| s.scroll.entry(panel as usize).or_default().1 = height);
        update_scrollbar(panel);
    }

    /// Calls `handle` for every event until it returns false or the window closes.
    pub fn run(&self, panel: HWND, timer: Option<u32>, mut handle: impl FnMut(Event) -> bool) {
        // SAFETY: standard message loop for our window.
        unsafe {
            if let Some(milliseconds) = timer {
                SetTimer(self.handle, 1, milliseconds, None);
            }
            ShowWindow(self.handle, SW_SHOW);
            // Paint everything once more: the first pass can miss the shapes of a panel.
            RedrawWindow(
                self.handle,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
            SetForegroundWindow(self.handle);
            // Start without focus marks; the dialog keys (Tab, arrows) switch them on.
            let hide = (UIS_SET | UISF_HIDEFOCUS << 16) as usize;
            SendMessageW(self.handle, WM_CHANGEUISTATE, hide, 0);
            // The first control with a tab stop.
            SetFocus(GetNextDlgTabItem(self.handle, null_mut(), 0));
            let mut focus = GetFocus();
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                // Enter presses the focused button; the dialog keys would always mean "save".
                if message.message == WM_KEYDOWN
                    && message.wParam == usize::from(VK_RETURN)
                    && is_push_button(GetFocus())
                {
                    SendMessageW(GetFocus(), BM_CLICK, 0, 0);
                } else if IsDialogMessageW(self.handle, &message) == 0 {
                    // Not a Tab, arrow, Enter (id 1) or Escape (id 2) key.
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if GetFocus() != focus {
                    focus = GetFocus();
                    reveal(panel, focus);
                }
                while let Some(event) = state(|s| s.events.pop_front()) {
                    if !handle(event) {
                        DestroyWindow(self.handle);
                        return;
                    }
                }
            }
        }
    }
}

fn is_push_button(control: HWND) -> bool {
    state(|s| {
        s.styles.get(&(control as usize)).is_some_and(|style| {
            matches!(style.look, Look::Primary | Look::Chip)
                || (matches!(style.look, Look::Secondary)
                    && s.buttons.contains(&(control as usize)))
        })
    })
}

pub fn text(control: HWND) -> String {
    // SAFETY: the buffer is as long as announced.
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(control) as usize + 1];
        let length = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length as usize])
    }
}

/// Changes the text only if it differs, to avoid flicker.
pub fn set_text(control: HWND, value: &str) {
    if text(control) != value {
        // SAFETY: NUL-terminated string.
        unsafe { SetWindowTextW(control, wide(value).as_ptr()) };
    }
}

pub fn checked(control: HWND) -> bool {
    // SAFETY: a plain query.
    unsafe { SendMessageW(control, BM_GETCHECK, 0, 0) == BST_CHECKED as isize }
}

pub fn set_checked(control: HWND, on: bool) {
    // SAFETY: a plain value message.
    unsafe { SendMessageW(control, BM_SETCHECK, usize::from(on), 0) };
}

pub fn selected(combo: HWND) -> Option<usize> {
    // SAFETY: a plain query.
    usize::try_from(unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) }).ok()
}

pub fn show(control: HWND, visible: bool) {
    // SAFETY: our own control.
    unsafe { ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE }) };
}

pub fn repaint(window: HWND) {
    // SAFETY: our own window; the background is painted again as well.
    unsafe { InvalidateRect(window, null(), 1) };
}

fn client(window: HWND) -> RECT {
    // SAFETY: fills the struct for a valid window.
    unsafe {
        let mut area: RECT = std::mem::zeroed();
        GetClientRect(window, &mut area);
        area
    }
}

fn update_scrollbar(panel: HWND) {
    let (position, content) =
        state(|s| s.scroll.get(&(panel as usize)).copied()).unwrap_or_default();
    let info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        nMin: 0,
        nMax: (content - 1).max(0),
        nPage: client(panel).bottom.max(0) as u32,
        nPos: position,
        nTrackPos: 0,
    };
    // SAFETY: our panel and a complete struct.
    unsafe { SetScrollInfo(panel, SB_VERT, &info, 1) };
}

fn scroll_to(panel: HWND, target: i32) {
    let visible = client(panel).bottom;
    let delta = state(|s| {
        let (position, content) = s.scroll.entry(panel as usize).or_default();
        let target = target.min(*content - visible).max(0);
        let delta = *position - target;
        *position = target;
        delta
    });
    if delta != 0 {
        // SAFETY: moves the panel's children together with its picture.
        unsafe {
            ScrollWindowEx(
                panel,
                0,
                delta,
                null(),
                null(),
                null_mut(),
                null_mut(),
                SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE,
            );
        }
    }
    update_scrollbar(panel);
}

fn scroll_position(panel: HWND) -> i32 {
    state(|s| s.scroll.get(&(panel as usize)).map_or(0, |p| p.0))
}

/// Scrolls so that a control which got the keyboard focus is visible.
fn reveal(panel: HWND, control: HWND) {
    // SAFETY: queries on windows of this thread.
    unsafe {
        if control.is_null() || IsChild(panel, control) == 0 {
            return;
        }
        let mut area: RECT = std::mem::zeroed();
        GetWindowRect(control, &mut area);
        let mut points = [
            windows_sys::Win32::Foundation::POINT {
                x: area.left,
                y: area.top,
            },
            windows_sys::Win32::Foundation::POINT {
                x: area.right,
                y: area.bottom,
            },
        ];
        MapWindowPoints(null_mut(), panel, points.as_mut_ptr(), 2);
        let (top, bottom, visible) = (points[0].y, points[1].y, client(panel).bottom);
        let margin = 12;
        if top < margin {
            scroll_to(panel, scroll_position(panel) + top - margin);
        } else if bottom > visible - margin {
            scroll_to(panel, scroll_position(panel) + bottom - visible + margin);
        }
    }
}

unsafe fn paint_background(window: HWND, dc: HDC) {
    let background =
        state(|s| s.styles.get(&(window as usize)).map(|s| s.background)).unwrap_or(BACKGROUND);
    let offset = scroll_position(window);
    let boxes = state(|s| s.boxes.get(&(window as usize)).cloned()).unwrap_or_default();
    // SAFETY: draws into the DC Windows passed and restores the selected objects.
    unsafe {
        FillRect(dc, &client(window), brush(background));
        for b in boxes.iter().filter(|b| b.visible) {
            let old_brush = SelectObject(dc, brush(b.fill));
            let pen = CreatePen(PS_SOLID, 1, BORDER);
            let old_pen = SelectObject(dc, pen);
            let radius = if b.fill == FIELD { 12 } else { 20 };
            RoundRect(
                dc,
                b.area.left,
                b.area.top - offset,
                b.area.right,
                b.area.bottom - offset,
                radius,
                radius,
            );
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            DeleteObject(pen);
        }
    }
}

/// Draws buttons, switches and check boxes; their behaviour stays that of Windows.
unsafe fn draw_button(draw: &NMCUSTOMDRAW) {
    let control = draw.hdr.hwndFrom;
    let Some(style) = state(|s| s.styles.get(&(control as usize)).copied()) else {
        return;
    };
    let (dc, area) = (draw.hdc, draw.rc);
    let on = checked(control);
    let hot = draw.uItemState & CDIS_HOT != 0;
    // Windows hides focus marks until the keyboard is used (see `Window::run`).
    // SAFETY: a plain query.
    let cues = unsafe { SendMessageW(control, WM_QUERYUISTATE, 0, 0) } as u32;
    let focus = draw.uItemState & CDIS_FOCUS != 0 && cues & UISF_HIDEFOCUS == 0;
    let height = area.bottom - area.top;
    smooth(dc, area, |dc, area, k| {
        // SAFETY: GDI drawing with objects that are restored and deleted again.
        unsafe {
            FillRect(dc, &area, brush(style.background));
            let rounded = |area: RECT, fill: u32, line: u32, radius: i32| {
                let pen = CreatePen(PS_SOLID, k, line);
                let old_pen = SelectObject(dc, pen);
                let old_brush = SelectObject(dc, brush(fill));
                RoundRect(
                    dc,
                    area.left,
                    area.top,
                    area.right,
                    area.bottom,
                    radius,
                    radius,
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(pen);
            };
            let height = area.bottom;
            match style.look {
                Look::Switch => {
                    let track = if on { ACCENT } else { BORDER };
                    let outline = if focus { TEXT } else { track };
                    rounded(area, track, outline, height);
                    let side = height - 8 * k;
                    let left = if on { area.right - side - 4 * k } else { 4 * k };
                    let knob = RECT {
                        left,
                        top: 4 * k,
                        right: left + side,
                        bottom: 4 * k + side,
                    };
                    let color = if on { BACKGROUND } else { MUTED };
                    rounded(knob, color, color, side);
                }
                Look::CheckBox => {
                    let line = if focus { TEXT } else { BORDER };
                    rounded(area, FIELD, line, 6 * k);
                    if on {
                        let pen = CreatePen(PS_SOLID, (height / 9).max(2), ACCENT);
                        let old = SelectObject(dc, pen);
                        let at = |x: i32, y: i32| (height * x / 20, height * y / 20);
                        let (x, y) = at(5, 10);
                        MoveToEx(dc, x, y, null_mut());
                        for (x, y) in [at(9, 14), at(15, 6)] {
                            LineTo(dc, x, y);
                        }
                        SelectObject(dc, old);
                        DeleteObject(pen);
                    }
                }
                Look::Primary | Look::Secondary | Look::Chip => {
                    let fill = button_fill(style.look, on, hot);
                    let line = match style.look {
                        _ if focus => TEXT,
                        Look::Secondary => BORDER,
                        _ => fill,
                    };
                    rounded(area, fill, line, 14 * k);
                }
            }
        }
    });
    if matches!(style.look, Look::Switch | Look::CheckBox) {
        return;
    }
    // The label goes on top at normal size, so that it stays crisp.
    let color = match style.look {
        Look::Primary => BACKGROUND,
        _ => style.text,
    };
    let label: Vec<u16> = text(control).encode_utf16().collect();
    // SAFETY: the length is passed explicitly; the font is restored.
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        let font = SendMessageW(control, WM_GETFONT, 0, 0) as HFONT;
        let old = SelectObject(dc, font);
        let mut area = draw.rc;
        area.bottom = area.top + height;
        DrawTextW(
            dc,
            label.as_ptr(),
            label.len() as i32,
            &mut area,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
        );
        SelectObject(dc, old);
    }
}

fn button_fill(look: Look, on: bool, hot: bool) -> u32 {
    match look {
        Look::Primary => ACCENT,
        Look::Chip if on => ACCENT_DARK,
        _ if hot => HOVER,
        _ => CARD,
    }
}

/// GDI does not smooth edges: draw `factor` times as large and let the downscaling do it.
/// `paint` gets the enlarged area, starting at 0/0, and that factor.
fn smooth(target: HDC, area: RECT, paint: impl FnOnce(HDC, RECT, i32)) {
    const FACTOR: i32 = 4;
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    let large = RECT {
        left: 0,
        top: 0,
        right: width * FACTOR,
        bottom: height * FACTOR,
    };
    // SAFETY: the memory DC and bitmap are restored and deleted below.
    unsafe {
        let dc = CreateCompatibleDC(target);
        let bitmap = CreateCompatibleBitmap(target, large.right, large.bottom);
        let old = SelectObject(dc, bitmap);
        paint(dc, large, FACTOR);
        SetStretchBltMode(target, HALFTONE);
        SetBrushOrgEx(target, 0, 0, null_mut());
        StretchBlt(
            target,
            area.left,
            area.top,
            width,
            height,
            dc,
            0,
            0,
            large.right,
            large.bottom,
            SRCCOPY,
        );
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
    }
}
/// Messages the main window and the panels handle alike.
unsafe fn common(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // SAFETY: lparam points to the structures documented for these messages.
    unsafe {
        match message {
            WM_ERASEBKGND => {
                paint_background(window, wparam as HDC);
                Some(1)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let style = state(|s| s.styles.get(&(lparam as usize)).copied());
                // The list of a drop-down is not one of our controls.
                let (text, background) = style.map_or((TEXT, FIELD), |s| (s.text, s.background));
                SetTextColor(wparam as HDC, text);
                SetBkColor(wparam as HDC, background);
                Some(brush(background) as LRESULT)
            }
            WM_NOTIFY => {
                let header = &*(lparam as *const NMHDR);
                if header.code != NM_CUSTOMDRAW {
                    return None;
                }
                let draw = &*(lparam as *const NMCUSTOMDRAW);
                if !state(|s| s.styles.contains_key(&(header.hwndFrom as usize))) {
                    return None;
                }
                // Erasing comes first; answering "skip" there would skip painting too.
                if draw.dwDrawStage != CDDS_PREPAINT {
                    return Some(CDRF_DODEFAULT as LRESULT);
                }
                draw_button(draw);
                Some(CDRF_SKIPDEFAULT as LRESULT)
            }
            WM_COMMAND => {
                let id = wparam as u16;
                let code = (wparam >> 16) as u32 & 0xFFFF;
                // Controls without an id report changes that nobody needs, except these.
                if id != 0 || code == EN_CHANGE || code == BN_CLICKED {
                    state(|s| s.events.push_back(Event::Command(id, code)));
                }
                Some(0)
            }
            _ => None,
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwards Windows' own parameters.
    unsafe {
        if let Some(result) = common(window, message, wparam, lparam) {
            return result;
        }
        match message {
            WM_TIMER => {
                state(|s| s.events.push_back(Event::Timer));
                0
            }
            WM_CLOSE => {
                state(|s| s.events.push_back(Event::Close));
                0
            }
            WM_SIZE => {
                state(|s| s.events.push_back(Event::Command(RESIZED, 0)));
                0
            }
            // The wheel goes to whatever has the focus; pass it on to the panel.
            WM_MOUSEWHEEL => {
                let mut child = GetWindow(window, GW_CHILD);
                while !child.is_null() {
                    if state(|s| s.scroll.contains_key(&(child as usize))) {
                        return SendMessageW(child, message, wparam, lparam);
                    }
                    child = GetWindow(child, GW_HWNDNEXT);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

/// Command id sent when the window changes its size.
pub const RESIZED: u16 = 0xFFF0;

unsafe extern "system" fn panel_proc(
    panel: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwards Windows' own parameters.
    unsafe {
        if let Some(result) = common(panel, message, wparam, lparam) {
            return result;
        }
        match message {
            WM_VSCROLL => {
                let mut info: SCROLLINFO = std::mem::zeroed();
                info.cbSize = size_of::<SCROLLINFO>() as u32;
                info.fMask = SIF_ALL;
                GetScrollInfo(panel, SB_VERT, &mut info);
                let page = info.nPage as i32;
                let target = match i32::from(wparam as u16) {
                    SB_LINEUP => info.nPos - 40,
                    SB_LINEDOWN => info.nPos + 40,
                    SB_PAGEUP => info.nPos - page,
                    SB_PAGEDOWN => info.nPos + page,
                    SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
                    SB_TOP => 0,
                    SB_BOTTOM => info.nMax,
                    _ => info.nPos,
                };
                scroll_to(panel, target);
                0
            }
            WM_MOUSEWHEEL => {
                let turned = i32::from((wparam >> 16) as u16 as i16);
                scroll_to(panel, scroll_position(panel) - turned / 2);
                0
            }
            WM_SIZE => {
                // Stay within the content when the panel grows.
                scroll_to(panel, scroll_position(panel));
                0
            }
            _ => DefWindowProcW(panel, message, wparam, lparam),
        }
    }
}
