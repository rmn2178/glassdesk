//! The dock's context menu, drawn like a macOS menu: frosted glass that
//! follows the light or dark theme, compact rows, and the selection in the
//! system accent color. Mouse and full keyboard control (arrows, Home/End,
//! Enter, Esc and the underlined letters of the native menu).
//!
//! With a screen reader or high contrast on, the native Windows menu is used
//! instead: it is what assistive technology knows how to read and recolor.
//! The menu exists only while open.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use nosih_shell::icons::IconImage;
use windows::{
    UI::{
        Color,
        Composition::{
            ContainerVisual, Desktop::DesktopWindowTarget, ShapeVisual, Visual, VisualCollection,
        },
    },
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMWA_BORDER_COLOR, DWMWA_USE_HOSTBACKDROPBRUSH, DWMWA_WINDOW_CORNER_PREFERENCE,
                DWMWCP_ROUND, DwmSetWindowAttribute,
            },
            Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
        },
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                MAPVK_VK_TO_CHAR, MapVirtualKeyW, ReleaseCapture, SetCapture, VK_DOWN, VK_END,
                VK_ESCAPE, VK_HOME, VK_RETURN, VK_SPACE, VK_UP,
            },
            WindowsAndMessaging::{
                AppendMenuW, CS_DROPSHADOW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW,
                IDC_ARROW, KillTimer, LoadCursorW, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
                MSG, PostMessageW, PostQuitMessage, RegisterClassW, SPI_GETHIGHCONTRAST,
                SPI_GETSCREENREADER, SW_SHOWNA, SWP_NOACTIVATE,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SetForegroundWindow, SetTimer,
                SetWindowPos, ShowWindow, SystemParametersInfoW, TPM_BOTTOMALIGN, TPM_CENTERALIGN,
                TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_RIGHTBUTTON,
                TPM_TOPALIGN, TPM_VCENTERALIGN, TRACK_POPUP_MENU_FLAGS, TrackPopupMenuEx,
                TranslateMessage, WA_INACTIVE, WM_ACTIVATE, WM_CANCELMODE, WM_CAPTURECHANGED,
                WM_CHAR, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE,
                WM_NULL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WM_TIMER,
                WM_XBUTTONDOWN, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::{BOOL, HSTRING, Interface, PCSTR, PCWSTR, w},
};

use windows_numerics::{Vector2, Vector3};

use crate::gfx::{Gfx, Ink, LabelBrush};

pub enum MenuItem {
    Action {
        command: u32,
        label: String,
        checked: bool,
        enabled: bool,
        /// Shown before the label in the drawn menu, as jump list entries.
        icon: Option<IconImage>,
    },
    /// The title of a section, such as a jump list's "Recent". Not selectable.
    Header(String),
    Separator,
}

impl MenuItem {
    pub fn action(command: u32, label: impl Into<String>) -> Self {
        Self::Action {
            command,
            label: label.into(),
            checked: false,
            enabled: true,
            icon: None,
        }
    }
}

/// Shows the menu on the `placement` side of `anchor` and returns the
/// selected command (`None` if dismissed).
///
/// Runs a modal loop that continues dispatching messages to dock windows.
/// The caller must not hold the app state while this function runs.
pub fn show(
    gfx: &Gfx,
    owner: HWND,
    items: &[MenuItem],
    anchor: POINT,
    dark: bool,
    placement: Placement,
) -> Option<u32> {
    show_at(gfx, owner, items, anchor, dark, placement)
}

pub fn show_below(
    gfx: &Gfx,
    owner: HWND,
    items: &[MenuItem],
    anchor: POINT,
    dark: bool,
) -> Option<u32> {
    show_at(gfx, owner, items, anchor, dark, Placement::Below)
}

/// Where a menu opens from its anchor point: above it, centred (an icon on
/// a bottom dock), below it, or beside it, centred vertically (an icon on a
/// side dock: to its right on a left dock, to its left on a right one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Above,
    Below,
    Right,
    Left,
}

impl Placement {
    /// The same placement for `TrackPopupMenuEx`.
    #[must_use]
    pub fn native_flags(self) -> TRACK_POPUP_MENU_FLAGS {
        match self {
            Self::Above => TPM_CENTERALIGN | TPM_BOTTOMALIGN,
            Self::Below => TPM_CENTERALIGN | TPM_TOPALIGN,
            Self::Right => TPM_LEFTALIGN | TPM_VCENTERALIGN,
            Self::Left => TPM_RIGHTALIGN | TPM_VCENTERALIGN,
        }
    }

    /// Top-left corner of a `w` x `h` menu placed from `anchor`, before
    /// keeping it on screen.
    fn corner(self, anchor: POINT, w: i32, h: i32) -> (i32, i32) {
        match self {
            Self::Above => (anchor.x - w / 2, anchor.y - h),
            Self::Below => (anchor.x - w / 2, anchor.y),
            Self::Right => (anchor.x, anchor.y - h / 2),
            Self::Left => (anchor.x - w, anchor.y - h / 2),
        }
    }
}

fn show_at(
    gfx: &Gfx,
    owner: HWND,
    items: &[MenuItem],
    anchor: POINT,
    dark: bool,
    placement: Placement,
) -> Option<u32> {
    if needs_native_menu() {
        return show_native(owner, items, anchor, dark, placement);
    }
    match Popup::open(gfx, owner, items, anchor, dark, placement) {
        Ok(popup) => popup.run(),
        Err(error) => {
            eprintln!("nosih: menú: {error:#}");
            show_native(owner, items, anchor, dark, placement)
        }
    }
}

/// A screen reader or high contrast is on.
fn needs_native_menu() -> bool {
    let mut reader = BOOL::default();
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: Each output is a valid structure of the size the call expects.
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETSCREENREADER,
            0,
            Some((&raw mut reader).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&raw mut contrast).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    reader.as_bool() || contrast.dwFlags.contains(HCF_HIGHCONTRASTON)
}

/* ------------------------------------------------------------ Drawn menu */

/// Sizes in logical pixels, after the macOS menu.
const PAD_Y: f32 = 5.0;
const ROW: f32 = 24.0;
const HEADER: f32 = 22.0;
const HEADER_FONT: f32 = 11.5;
const HEADER_OPACITY: f32 = 0.55;
const SEPARATOR: f32 = 11.0;
/// The selection is inset from the menu's sides and rounded.
const INSET: f32 = 5.0;
const SELECTION_RADIUS: f32 = 5.0;
/// Text starts after room for a check mark.
const TEXT_LEFT: f32 = 24.0;
/// With icons: the icon where the check mark goes, then the text.
const ICON: f32 = 16.0;
const ICON_LEFT: f32 = 4.0;
const ICON_TEXT_LEFT: f32 = 32.0;
/// The gray white glyphs take on a light menu, near the text's.
const DARK_GLYPH: u8 = 0x1F;
const TEXT_RIGHT: f32 = 24.0;
const FONT: f32 = 13.5;
const MIN_WIDTH: f32 = 180.0;
/// Space kept between the menu and the screen's edges.
const MARGIN: f32 = 6.0;
/// The release of the click that opened the menu must not pick an item.
const ARM_DELAY: Duration = Duration::from_millis(300);
/// Checks that the menu still has the focus, in case Windows gave it away
/// without telling (it then could not see clicks elsewhere to close).
const FOCUS_TIMER: usize = 1;
const CHECK_MARK: char = '\u{E73E}';

/// Set by the window procedure when the menu loses the focus or the mouse.
static DISMISSED: AtomicBool = AtomicBool::new(false);

struct Row {
    /// `None` for a separator.
    command: Option<u32>,
    enabled: bool,
    mnemonic: Option<char>,
    top: f32,
    height: f32,
    /// The label in the theme's ink, and in white over the selection.
    normal: Option<Visual>,
    selected: Option<Visual>,
}

struct Popup {
    hwnd: HWND,
    owner: HWND,
    _target: DesktopWindowTarget,
    rows: Vec<Row>,
    selection: ShapeVisual,
    width: f32,
    height: f32,
    scale: f32,
    hot: Option<usize>,
    opened: Instant,
    armed: bool,
    previous: HWND,
}

impl Popup {
    fn open(
        gfx: &Gfx,
        owner: HWND,
        items: &[MenuItem],
        anchor: POINT,
        dark: bool,
        placement: Placement,
    ) -> Result<Self> {
        let c = &gfx.compositor;
        // SAFETY: Read-only query on our own window.
        let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
        let px = |logical: f32| (logical * scale).round();
        let ink = if dark { Ink::Light } else { Ink::Dark };

        // Labels first: they decide the width.
        struct Label {
            normal: LabelBrush,
            selected: LabelBrush,
        }
        let mut labels: Vec<Option<Label>> = Vec::with_capacity(items.len());
        let mut headers: Vec<Option<LabelBrush>> = Vec::with_capacity(items.len());
        let mut text_width: f32 = 0.0;
        for item in items {
            headers.push(match item {
                MenuItem::Header(title) => {
                    let brush = gfx.text_brush_sized(title, scale, HEADER_FONT, true, ink)?;
                    text_width = text_width.max(brush.width);
                    Some(brush)
                }
                _ => None,
            });
            labels.push(match item {
                MenuItem::Separator | MenuItem::Header(_) => None,
                MenuItem::Action { label, .. } => {
                    let (text, _) = strip_mnemonic(label);
                    let normal = gfx.text_brush_sized(&text, scale, FONT, false, ink)?;
                    let selected = gfx.text_brush_sized(&text, scale, FONT, false, Ink::Light)?;
                    text_width = text_width.max(normal.width);
                    Some(Label { normal, selected })
                }
            });
        }
        // Room for icons only when some row has one.
        let has_icons = items
            .iter()
            .any(|item| matches!(item, MenuItem::Action { icon: Some(_), .. }));
        let text_left = px(if has_icons { ICON_TEXT_LEFT } else { TEXT_LEFT });
        let width = (text_width + text_left + px(TEXT_RIGHT)).max(px(MIN_WIDTH));
        let mut height = px(PAD_Y) * 2.0;
        for item in items {
            height += px(match item {
                MenuItem::Separator => SEPARATOR,
                MenuItem::Header(_) => HEADER,
                MenuItem::Action { .. } => ROW,
            });
        }

        let glass = nosih_shell::system::transparency_enabled();
        let root = c.CreateContainerVisual()?;
        root.SetSize(Vector2::new(width, height))?;
        let layers = root.Children()?;
        draw_popup_background(&layers, dark, glass, width, height)?;

        let selection = c.CreateShapeVisual()?;
        let selection_shape = c.CreateRoundedRectangleGeometry()?;
        selection_shape.SetSize(Vector2::new(width - px(INSET) * 2.0, px(ROW)))?;
        selection_shape
            .SetCornerRadius(Vector2::new(px(SELECTION_RADIUS), px(SELECTION_RADIUS)))?;
        let fill = c.CreateSpriteShapeWithGeometry(&selection_shape)?;
        fill.SetFillBrush(&c.CreateColorBrushWithColor(accent())?)?;
        selection.Shapes()?.Append(&fill)?;
        selection.SetSize(Vector2::new(width, px(ROW)))?;
        selection.SetOffset(Vector3::new(px(INSET), 0.0, 0.0))?;
        selection.SetIsVisible(false)?;
        layers.InsertAtTop(&selection)?;

        let line = if dark {
            rgba(255, 255, 255, 32)
        } else {
            rgba(0, 0, 0, 28)
        };
        let mut rows = Vec::with_capacity(items.len());
        let mut top = px(PAD_Y);
        for ((item, label), header) in items.iter().zip(labels).zip(headers) {
            match (item, label) {
                (
                    MenuItem::Action {
                        command,
                        label,
                        checked,
                        enabled,
                        icon,
                    },
                    Some(brushes),
                ) => {
                    let row_height = px(ROW);
                    let place = |brush: &LabelBrush| -> Result<Visual> {
                        let group = c.CreateContainerVisual()?;
                        let text_top = top + ((row_height - brush.height) / 2.0).round();
                        add_sprite(
                            &group.Children()?,
                            brush.brush.cast()?,
                            text_left,
                            text_top,
                            brush.width,
                            brush.height,
                        )?;
                        group.SetOpacity(if *enabled { 1.0 } else { 0.35 })?;
                        layers.InsertAtTop(&group)?;
                        Ok(group.cast()?)
                    };
                    let normal = place(&brushes.normal)?;
                    let selected = place(&brushes.selected)?;
                    selected.SetIsVisible(false)?;
                    if let Some(icon) = icon {
                        let side = px(ICON);
                        // A white glyph made for a dark taskbar (Edge's tasks)
                        // takes the text's ink on a light menu; over the
                        // selection it stays white, as the text does.
                        let normal_icon =
                            (!dark && is_white_glyph(icon)).then(|| tinted(icon, DARK_GLYPH));
                        let pairs = [
                            (&normal, normal_icon.as_ref().unwrap_or(icon)),
                            (&selected, icon),
                        ];
                        for (visual, image) in pairs {
                            // A bad icon costs only the icon, not the menu.
                            let Ok(brush) = gfx.icon_brush(image, side as u32) else {
                                continue;
                            };
                            let group: ContainerVisual = visual.cast()?;
                            add_sprite(
                                &group.Children()?,
                                brush.cast()?,
                                px(INSET) + px(ICON_LEFT),
                                top + ((row_height - side) / 2.0).round(),
                                side,
                                side,
                            )?;
                        }
                    }
                    if *checked {
                        for (visual, mark_ink) in [(&normal, ink), (&selected, Ink::Light)] {
                            let mark =
                                gfx.glyph_brush(&[(CHECK_MARK, 1.0)], scale * 0.75, mark_ink)?;
                            let group: ContainerVisual = visual.cast()?;
                            add_sprite(
                                &group.Children()?,
                                mark.brush.cast()?,
                                px(INSET) + px(3.0),
                                top + ((row_height - mark.height) / 2.0).round(),
                                mark.width,
                                mark.height,
                            )?;
                        }
                    }
                    rows.push(Row {
                        command: Some(*command),
                        enabled: *enabled,
                        mnemonic: strip_mnemonic(label).1,
                        top,
                        height: row_height,
                        normal: Some(normal),
                        selected: Some(selected),
                    });
                    top += row_height;
                }
                (MenuItem::Header(_), _) => {
                    let row_height = px(HEADER);
                    if let Some(brush) = header {
                        let group = c.CreateContainerVisual()?;
                        // Sits low in its row, near the items it names.
                        add_sprite(
                            &group.Children()?,
                            brush.brush.cast()?,
                            px(INSET) + px(7.0),
                            top + (row_height - brush.height - px(2.0)).round(),
                            brush.width,
                            brush.height,
                        )?;
                        group.SetOpacity(HEADER_OPACITY)?;
                        layers.InsertAtTop(&group)?;
                    }
                    rows.push(Row {
                        command: None,
                        enabled: false,
                        mnemonic: None,
                        top,
                        height: row_height,
                        normal: None,
                        selected: None,
                    });
                    top += row_height;
                }
                _ => {
                    let row_height = px(SEPARATOR);
                    add_sprite(
                        &layers,
                        c.CreateColorBrushWithColor(line)?.cast()?,
                        px(INSET) + px(5.0),
                        top + (row_height / 2.0).floor(),
                        width - (px(INSET) + px(5.0)) * 2.0,
                        scale.max(1.0).floor(),
                    )?;
                    rows.push(Row {
                        command: None,
                        enabled: false,
                        mnemonic: None,
                        top,
                        height: row_height,
                        normal: None,
                        selected: None,
                    });
                    top += row_height;
                }
            }
        }

        // Placed from the anchor, and inside its monitor.
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `monitor` is a valid structure with its size set.
        unsafe {
            let _ = GetMonitorInfoW(
                MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            );
        }
        let screen: RECT = monitor.rcMonitor;
        let (w, h) = (width as i32, height as i32);
        let margin = px(MARGIN) as i32;
        let (left, top) = placement.corner(anchor, w, h);
        let left = left.clamp(
            screen.left + margin,
            (screen.right - margin - w).max(screen.left + margin),
        );
        let top = top.clamp(
            screen.top + margin,
            (screen.bottom - margin - h).max(screen.top + margin),
        );

        let hwnd = create_window(dark)?;
        // SAFETY: `hwnd` is the live window created above.
        let target = unsafe {
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        target.SetRoot(&root)?;

        DISMISSED.store(false, Ordering::Relaxed);
        // SAFETY: Our own window; the previous foreground is restored on close.
        let previous = unsafe {
            let previous = GetForegroundWindow();
            SetWindowPos(hwnd, None, left, top, w, h, SWP_NOACTIVATE)?;
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            // In front with the focus, so keys reach it and clicks elsewhere
            // close it, as the native menu does.
            let _ = SetForegroundWindow(hwnd);
            SetCapture(hwnd);
            SetTimer(Some(hwnd), FOCUS_TIMER, 250, None);
            previous
        };
        Ok(Self {
            hwnd,
            owner,
            _target: target,
            rows,
            selection,
            width,
            height,
            scale,
            hot: None,
            opened: Instant::now(),
            armed: false,
            previous,
        })
    }

    /// The modal loop: input for the menu is handled here; everything else
    /// is dispatched, so the dock keeps animating.
    fn run(mut self) -> Option<u32> {
        let mut chosen = None;
        let mut msg = MSG::default();
        loop {
            if DISMISSED.load(Ordering::Relaxed) {
                break;
            }
            // SAFETY: Standard message retrieval into a valid MSG.
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if got.0 <= 0 {
                if got.0 == 0 {
                    // Leave WM_QUIT for the dock's own loop.
                    // SAFETY: Re-posts the quit request on this thread.
                    unsafe { PostQuitMessage(msg.wParam.0 as i32) };
                }
                break;
            }
            match self.handle(&msg) {
                Handled::No => {
                    // SAFETY: Dispatching a message retrieved above.
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                Handled::Yes => {}
                Handled::Close(command) => {
                    chosen = command;
                    break;
                }
            }
        }
        self.close();
        chosen
    }

    fn handle(&mut self, msg: &MSG) -> Handled {
        let x = (msg.lParam.0 & 0xFFFF) as i16 as f32;
        let y = ((msg.lParam.0 >> 16) & 0xFFFF) as i16 as f32;
        let own = msg.hwnd == self.hwnd;
        match msg.message {
            WM_TIMER if own && msg.wParam.0 == FOCUS_TIMER => {
                // SAFETY: Read-only query.
                let front = unsafe { GetForegroundWindow() };
                if front != self.hwnd && self.opened.elapsed() > ARM_DELAY {
                    return Handled::Close(None);
                }
                Handled::Yes
            }
            WM_MOUSEMOVE if own => {
                let hot = self.row_at(x, y);
                if hot.is_some() || !self.inside(x, y) {
                    self.set_hot(hot);
                }
                if self.opened.elapsed() > ARM_DELAY {
                    self.armed = true;
                }
                Handled::Yes
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN if own => {
                if !self.inside(x, y) {
                    return Handled::Close(None);
                }
                self.armed = true;
                Handled::Yes
            }
            WM_LBUTTONUP | WM_RBUTTONUP if own => {
                if !self.armed && self.opened.elapsed() < ARM_DELAY {
                    return Handled::Yes;
                }
                match self.row_at(x, y) {
                    Some(index) => Handled::Close(self.rows[index].command),
                    None => Handled::Yes,
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN if own => self.key(msg.wParam.0 as u16),
            // Letters are handled from WM_KEYDOWN; drop the characters.
            WM_CHAR if own => Handled::Yes,
            _ => Handled::No,
        }
    }

    fn key(&mut self, key: u16) -> Handled {
        match key {
            k if k == VK_ESCAPE.0 => Handled::Close(None),
            k if k == VK_RETURN.0 || k == VK_SPACE.0 => match self.hot {
                Some(index) => Handled::Close(self.rows[index].command),
                None => Handled::Yes,
            },
            k if k == VK_DOWN.0 => {
                self.step(1);
                Handled::Yes
            }
            k if k == VK_UP.0 => {
                self.step(-1);
                Handled::Yes
            }
            k if k == VK_HOME.0 => {
                self.hot = None;
                self.step(1);
                Handled::Yes
            }
            k if k == VK_END.0 => {
                self.hot = None;
                self.step(-1);
                Handled::Yes
            }
            _ => {
                // SAFETY: Pure key translation.
                let typed = unsafe { MapVirtualKeyW(u32::from(key), MAPVK_VK_TO_CHAR) };
                let Some(typed) =
                    char::from_u32(typed & 0xFFFF).map(|c| c.to_lowercase().next().unwrap_or(c))
                else {
                    return Handled::Yes;
                };
                // As the native menu: a letter picks the item it underlines.
                let matches: Vec<usize> = self
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| row.enabled && row.mnemonic == Some(typed))
                    .map(|(index, _)| index)
                    .collect();
                match matches.as_slice() {
                    [only] => Handled::Close(self.rows[*only].command),
                    [] => Handled::Yes,
                    many => {
                        let next = many
                            .iter()
                            .copied()
                            .find(|&index| self.hot.is_none_or(|hot| index > hot))
                            .unwrap_or(many[0]);
                        self.set_hot(Some(next));
                        Handled::Yes
                    }
                }
            }
        }
    }

    /// Moves the selection to the next enabled item, wrapping around.
    fn step(&mut self, by: isize) {
        let count = self.rows.len() as isize;
        let mut index = self
            .hot
            .map_or(if by > 0 { -1 } else { count }, |hot| hot as isize);
        for _ in 0..count {
            index = (index + by).rem_euclid(count);
            if self.rows[index as usize].enabled {
                self.set_hot(Some(index as usize));
                return;
            }
        }
    }

    fn inside(&self, x: f32, y: f32) -> bool {
        x >= 0.0 && y >= 0.0 && x < self.width && y < self.height
    }

    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.inside(x, y) {
            return None;
        }
        self.rows
            .iter()
            .position(|row| row.enabled && y >= row.top && y < row.top + row.height)
    }

    fn set_hot(&mut self, hot: Option<usize>) {
        if hot == self.hot {
            return;
        }
        let show = |row: &Row, selected: bool| {
            if let (Some(normal), Some(on)) = (&row.normal, &row.selected) {
                let _ = normal.SetIsVisible(!selected);
                let _ = on.SetIsVisible(selected);
            }
        };
        if let Some(old) = self.hot.and_then(|index| self.rows.get(index)) {
            show(old, false);
        }
        self.hot = hot;
        match hot.and_then(|index| self.rows.get(index)) {
            Some(row) => {
                show(row, true);
                let inset = (INSET * self.scale).round();
                let _ = self.selection.SetOffset(Vector3::new(inset, row.top, 0.0));
                let _ = self.selection.SetIsVisible(true);
            }
            None => {
                let _ = self.selection.SetIsVisible(false);
            }
        }
    }

    fn close(self) {
        // SAFETY: Our own window, destroyed once; the owner gets a message
        // so its loop notices the menu is gone, as with the native menu.
        unsafe {
            let foreground = GetForegroundWindow();
            let restore = foreground == self.hwnd || foreground == self.owner;
            let _ = KillTimer(Some(self.hwnd), FOCUS_TIMER);
            let _ = ReleaseCapture();
            let _ = DestroyWindow(self.hwnd);
            let _ = PostMessageW(Some(self.owner), WM_NULL, WPARAM(0), LPARAM(0));
            if restore {
                // Restore after a selection as well, but never override a
                // different app the user activated while dismissing the menu.
                let _ = SetForegroundWindow(self.previous);
            }
        }
    }
}

enum Handled {
    No,
    Yes,
    Close(Option<u32>),
}

fn create_window(dark: bool) -> Result<HWND> {
    static CLASS: OnceLock<()> = OnceLock::new();
    // SAFETY: The class is registered once with a procedure that lives for
    // the whole process; the window is configured before it is shown.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("NosihMenu");
        CLASS.get_or_init(|| {
            RegisterClassW(&WNDCLASSW {
                // The system shadow under the menu, as native menus have.
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(menu_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            });
        });
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
            w!("Nosih"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        apply_popup_frame(hwnd, dark);
        Ok(hwnd)
    }
}

/// Applies the native frame used by dock popups. Returns whether DWM
/// accepted the corner preference so callers can provide a fallback.
pub(crate) fn apply_popup_frame(hwnd: HWND, dark: bool) -> bool {
    let enable = BOOL::from(true);
    let round = DWMWCP_ROUND;
    let border = if dark {
        COLORREF(0x0058_5654)
    } else {
        COLORREF(0x00C8_C6C4)
    };
    // SAFETY: Attribute values have the expected type and size; hwnd is a
    // live window owned by the caller. Older systems can reject attributes.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            (&raw const enable).cast(),
            size_of_val(&enable) as u32,
        );
        let rounded = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const round).cast(),
            size_of_val(&round) as u32,
        )
        .is_ok();
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const border).cast(),
            size_of_val(&border) as u32,
        );
        rounded
    }
}

pub(crate) fn draw_popup_background(
    layers: &VisualCollection,
    dark: bool,
    glass: bool,
    width: f32,
    height: f32,
) -> Result<()> {
    let tint = popup_tint(dark, glass);
    let compositor = layers.Compositor()?;
    if glass {
        add_sprite(
            layers,
            compositor.CreateHostBackdropBrush()?.cast()?,
            0.0,
            0.0,
            width,
            height,
        )?;
    }
    add_sprite(
        layers,
        compositor.CreateColorBrushWithColor(tint)?.cast()?,
        0.0,
        0.0,
        width,
        height,
    )
}

pub(crate) fn popup_tint(dark: bool, glass: bool) -> Color {
    match (dark, glass) {
        (true, true) => rgba(38, 38, 40, 175),
        (false, true) => rgba(246, 246, 248, 215),
        (true, false) => rgba(44, 44, 46, 255),
        (false, false) => rgba(240, 240, 242, 255),
    }
}

unsafe extern "system" fn menu_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ACTIVATE if (w.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            DISMISSED.store(true, Ordering::Relaxed);
        }
        WM_CAPTURECHANGED if l.0 != hwnd.0 as isize => {
            DISMISSED.store(true, Ordering::Relaxed);
        }
        WM_CANCELMODE => DISMISSED.store(true, Ordering::Relaxed),
        WM_SETCURSOR => {
            // SAFETY: The stock arrow cursor is owned by Windows.
            if let Ok(arrow) = unsafe { LoadCursorW(None, IDC_ARROW) } {
                unsafe { SetCursor(Some(arrow)) };
            }
            return LRESULT(1);
        }
        _ => {}
    }
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// `true` for an icon drawn in white only: every visible pixel near white.
/// Pixels are premultiplied, so white is each channel close to its alpha.
fn is_white_glyph(icon: &IconImage) -> bool {
    let visible: Vec<&[u8; 4]> = icon
        .bgra
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 32)
        .collect();
    !visible.is_empty()
        && visible.iter().all(|pixel| {
            let alpha = u16::from(pixel[3]);
            pixel[..3]
                .iter()
                .all(|&channel| u16::from(channel) * 255 >= alpha * 215)
        })
}

/// `icon` in one gray `value`, keeping its shape (alpha).
fn tinted(icon: &IconImage, value: u8) -> IconImage {
    let mut bgra = icon.bgra.clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        let channel = (u16::from(value) * u16::from(pixel[3]) / 255) as u8;
        pixel[..3].fill(channel);
    }
    IconImage {
        width: icon.width,
        height: icon.height,
        bgra,
    }
}

/// The label without its `&` marker, and the letter it marked.
fn strip_mnemonic(label: &str) -> (String, Option<char>) {
    let mut text = String::with_capacity(label.len());
    let mut mnemonic = None;
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c != '&' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('&') => text.push('&'),
            Some(next) => {
                mnemonic.get_or_insert(next.to_lowercase().next().unwrap_or(next));
                text.push(next);
            }
            None => {}
        }
    }
    (text, mnemonic)
}

/// The user's accent color, as macOS highlights menus with its own.
fn accent() -> Color {
    nosih_shell::system::accent_color()
        .map_or(rgba(10, 132, 255, 255), |[r, g, b]| rgba(r, g, b, 255))
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn add_sprite(
    layers: &windows::UI::Composition::VisualCollection,
    brush: windows::UI::Composition::CompositionBrush,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Result<()> {
    let compositor = layers.Compositor()?;
    let sprite = compositor.CreateSpriteVisual()?;
    sprite.SetBrush(&brush)?;
    sprite.SetOffset(Vector3::new(x, y, 0.0))?;
    sprite.SetSize(Vector2::new(width, height))?;
    layers.InsertAtTop(&sprite)?;
    Ok(())
}

/* ---------------------------------------------------------- Native menu */

fn show_native(
    owner: HWND,
    items: &[MenuItem],
    anchor: POINT,
    dark: bool,
    placement: Placement,
) -> Option<u32> {
    apply_theme(dark);
    // SAFETY: The menu is destroyed before returning, and strings live through each call.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            let _ = match item {
                MenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                MenuItem::Header(title) => AppendMenuW(
                    menu,
                    MF_STRING | MF_GRAYED,
                    0,
                    &HSTRING::from(title.replace('&', "&&")),
                ),
                MenuItem::Action {
                    command,
                    label,
                    checked,
                    enabled,
                    ..
                } => {
                    let mut flags = MF_STRING;
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    AppendMenuW(
                        menu,
                        flags,
                        *command as usize,
                        &HSTRING::from(label.as_str()),
                    )
                }
            };
        }

        // Without making the window foreground, TrackPopupMenu does not dismiss
        // the menu when the user clicks outside it.
        let previous = GetForegroundWindow();
        let _ = SetForegroundWindow(owner);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | placement.native_flags()).0,
            anchor.x,
            anchor.y,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        let chosen = u32::try_from(chosen.0).ok().filter(|&command| command != 0);
        if GetForegroundWindow() == owner {
            let _ = SetForegroundWindow(previous);
        }
        chosen
    }
}

/// Windows has no public API for dark Win32 menus, so this uses `uxtheme.dll`
/// functions by ordinal, as Windows Terminal and Notepad++ do. If unavailable
/// on another Windows version, the menu remains light.
pub fn apply_theme(dark: bool) {
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;

    static FUNCTIONS: OnceLock<Option<(SetPreferredAppMode, FlushMenuThemes)>> = OnceLock::new();
    let functions = FUNCTIONS.get_or_init(|| {
        // SAFETY: Functions exported at these ordinals have these signatures
        // since Windows 10 1903, and the library is never unloaded.
        unsafe {
            let module = LoadLibraryW(w!("uxtheme.dll")).ok()?;
            let set = GetProcAddress(module, PCSTR(135 as *const u8))?;
            let flush = GetProcAddress(module, PCSTR(136 as *const u8))?;
            Some((
                std::mem::transmute::<unsafe extern "system" fn() -> isize, SetPreferredAppMode>(
                    set,
                ),
                std::mem::transmute::<unsafe extern "system" fn() -> isize, FlushMenuThemes>(flush),
            ))
        }
    });
    if let Some((set, flush)) = functions {
        // SAFETY: The function is resolved from the loaded module using its documented ordinal.
        unsafe {
            set(if dark { FORCE_DARK } else { FORCE_LIGHT });
            flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(pixels: &[[u8; 4]]) -> IconImage {
        IconImage {
            width: pixels.len() as u32,
            height: 1,
            bgra: pixels.concat(),
        }
    }

    #[test]
    fn only_white_glyphs_are_tinted_and_keep_their_shape() {
        // Premultiplied: half-transparent white is 128 in every channel.
        let glyph = icon(&[[255, 255, 255, 255], [128, 128, 128, 128], [0, 0, 0, 0]]);
        assert!(is_white_glyph(&glyph));
        assert!(!is_white_glyph(&icon(&[
            [255, 255, 255, 255],
            [200, 40, 0, 255]
        ])));
        assert!(!is_white_glyph(&icon(&[[0, 0, 0, 0]])));
        let dark = tinted(&glyph, 0x1F);
        assert_eq!(dark.bgra[..4], [0x1F, 0x1F, 0x1F, 255]);
        assert_eq!(dark.bgra[7], 128);
        assert_eq!(dark.bgra[8..], [0, 0, 0, 0]);
    }

    #[test]
    #[ignore = "Requires an interactive Windows desktop; briefly opens a menu"]
    fn keyboard_menu_opens_below_its_anchor_and_returns_the_selected_layout() -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::GetWindowRect,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let owner = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Nosih menu test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        };
        let result = (|| -> Result<()> {
            let anchor = POINT { x: 600, y: 60 };
            let previous = unsafe { GetForegroundWindow() };
            let popup = Popup::open(
                &gfx,
                owner,
                &[
                    MenuItem::action(1, "Español"),
                    MenuItem::action(2, "English"),
                ],
                anchor,
                true,
                Placement::Below,
            )?;
            let mut bounds = RECT::default();
            unsafe {
                GetWindowRect(popup.hwnd, &mut bounds)?;
                // Exercise the popup's real modal path without injecting any
                // keyboard input into the user's application.
                PostMessageW(
                    Some(popup.hwnd),
                    WM_KEYDOWN,
                    WPARAM(VK_DOWN.0 as usize),
                    LPARAM(0),
                )?;
                PostMessageW(
                    Some(popup.hwnd),
                    WM_KEYDOWN,
                    WPARAM(VK_RETURN.0 as usize),
                    LPARAM(0),
                )?;
            }
            let chosen = popup.run();
            assert_eq!(bounds.top, anchor.y);
            assert_eq!(chosen, Some(1));
            assert_eq!(unsafe { GetForegroundWindow() }, previous);
            Ok(())
        })();
        unsafe {
            let _ = DestroyWindow(owner);
        }
        result
    }

    #[test]
    fn strip_mnemonic_should_drop_the_marker_and_keep_its_letter() {
        assert_eq!(
            strip_mnemonic("Mostrar &escritorio"),
            ("Mostrar escritorio".to_owned(), Some('e'))
        );
        assert_eq!(
            strip_mnemonic("Show &Desktop"),
            ("Show Desktop".to_owned(), Some('d'))
        );
        assert_eq!(strip_mnemonic("A && B"), ("A & B".to_owned(), None));
    }
}
