//! Open windows and the apps they belong to.
//!
//! An app identifier is a shell name that can be opened and resolved to an icon
//! and display name: the lowercase executable path or, for packaged (Store) apps
//! and web apps that Windows lists on their own, `shell:AppsFolder\<AppUserModelID>`.

use std::collections::HashMap;

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWM_CLOAKED_SHELL, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
                DwmGetWindowAttribute,
            },
            Gdi::ClientToScreen,
        },
        Storage::{
            EnhancedStorage::PKEY_AppUserModel_ID, Packaging::Appx::GetApplicationUserModelId,
        },
        System::Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::{
            Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
            WindowsAndMessaging::{
                EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect,
                GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
                IsWindowVisible, IsZoomed, PostMessageW, SM_CXPADDEDBORDER, SM_CXSIZEFRAME,
                SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
                SetWindowPos, WINDOWPLACEMENT, WM_CLOSE, WPF_RESTORETOMAXIMIZED, WS_EX_APPWINDOW,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MAXIMIZE, WS_THICKFRAME,
            },
        },
    },
    core::{BOOL, PWSTR},
};

pub(crate) const APPS_FOLDER: &str = "shell:AppsFolder\\";
const FRAME_HOST_CLASS: &str = "ApplicationFrameWindow";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedWindow {
    pub hwnd: isize,
    pub app_id: String,
    pub minimized: bool,
}

#[must_use]
pub fn normalize_id(id: &str) -> String {
    // Shortcuts kept as pins (see `pins::shortcut_id`) keep their case: the
    // shell shows their file name as the label.
    if id.starts_with(APPS_FOLDER) || is_shortcut(id) {
        id.to_owned()
    } else {
        id.to_lowercase()
    }
}

/// Lists app windows and tracks which app each window belongs to.
///
/// Identifying a window's app requires opening its process or querying shell
/// properties, which makes each lookup expensive. The cache limits this to once
/// per window, when it appears.
#[derive(Debug, Default)]
pub struct WindowTracker {
    app_ids: HashMap<isize, String>,
}

impl WindowTracker {
    pub fn list(&mut self) -> Vec<TrackedWindow> {
        self.list_with_workspaces(false)
    }

    /// Optionally includes shell-cloaked windows on other Windows/tiling-WM
    /// workspaces. App-cloaked and inherited-cloak windows remain excluded.
    pub fn list_with_workspaces(&mut self, show_other_workspaces: bool) -> Vec<TrackedWindow> {
        unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
            // SAFETY: `list` points to the Vec passed by `list` and remains
            // alive for the entire synchronous enumeration.
            unsafe { (*(list.0 as *mut Vec<HWND>)).push(hwnd) };
            true.into()
        }

        let mut all: Vec<HWND> = Vec::new();
        // SAFETY: The Vec pointer remains valid until EnumWindows returns.
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&raw mut all as isize));
        }

        let windows: Vec<TrackedWindow> = all
            .into_iter()
            .filter(|&hwnd| is_app_window(hwnd, show_other_workspaces))
            .filter_map(|hwnd| {
                let key = hwnd.0 as isize;
                let app_id = match self.app_ids.get(&key) {
                    Some(id) => id.clone(),
                    // The newly created UWP app has not declared an identifier
                    // yet; do not cache it so the lookup can be retried.
                    None => {
                        let id = app_id(hwnd)?;
                        self.app_ids.insert(key, id.clone());
                        id
                    }
                };
                Some(TrackedWindow {
                    hwnd: key,
                    app_id,
                    minimized: unsafe { IsIconic(hwnd).as_bool() },
                })
            })
            .collect();

        // Remove closed windows because Windows reuses their handles.
        self.app_ids
            .retain(|hwnd, _| windows.iter().any(|window| window.hwnd == *hwnd));
        windows
    }
}

#[must_use]
pub fn title(hwnd: isize) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: The buffer remains alive during the call, which returns the number of characters written.
    let len = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// Restored-state rectangle of a window, also valid while it is minimized.
/// Workspace coordinates match screen coordinates here: the work area starts at
/// the screen's top-left corner because the dock reserves only the bottom edge.
#[must_use]
pub fn normal_rect(hwnd: isize) -> Option<RECT> {
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: The structure has its size initialized and lives for the call.
    unsafe { GetWindowPlacement(HWND(hwnd as *mut _), &mut placement) }.ok()?;
    // Placement uses workspace coordinates, whose origin is the top-left
    // corner of the work area: below the menu bar when it is shown.
    let (dx, dy) = crate::system::primary_monitor().map_or((0, 0), |monitor| {
        (
            monitor.work_left - monitor.screen.left,
            monitor.work_top - monitor.screen.top,
        )
    });
    let local = placement.rcNormalPosition;
    let rect = RECT {
        left: local.left + dx,
        top: local.top + dy,
        right: local.right + dx,
        bottom: local.bottom + dy,
    };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

/// Where a window is on screen now, and whether it is maximized; `None`
/// while it is minimized. Taken whenever the dock looks at its windows, so a
/// minimize knows where a snapped window was: Windows keeps a snapped
/// window's pre-snap rectangle as its "normal" one.
#[must_use]
pub fn screen_rect(hwnd: isize) -> Option<(RECT, bool)> {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Read-only queries on a window handle.
    unsafe {
        if IsIconic(window).as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok()?;
        Some((rect, IsZoomed(window).as_bool()))
    }
}

/// The part of a window that shows on screen: its frame without the
/// invisible resize borders Windows 10 and 11 put around it (some 7 px on
/// each side but the top at 100 %), which `screen_rect` includes. `None`
/// while it is minimized.
#[must_use]
pub fn visible_rect(hwnd: isize) -> Option<RECT> {
    let window = HWND(hwnd as *mut _);
    let (whole, _) = screen_rect(hwnd)?;
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid output of the attribute's size.
    let framed = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    Some(if framed.is_ok() { rect } else { whole })
}

/// Whether the program `app_id` (an executable path, as `normalize_id`
/// writes it) has a process running, windows or not: an app that lives in
/// the notification area, such as Rainmeter. Packaged apps are not looked
/// up: always `false`.
///
/// One process snapshot; only processes with the same file name are opened.
#[must_use]
pub fn is_running(app_id: &str) -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    if app_id.starts_with(APPS_FOLDER) {
        return false;
    }
    let Some(name) = std::path::Path::new(app_id)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
    else {
        return false;
    };
    // SAFETY: The snapshot is closed below; `entry` has its size set and
    // belongs to it.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more && !found {
            let len = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase() == name {
                found = process_image(entry.th32ProcessID)
                    .is_some_and(|image| normalize_id(&image) == app_id);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

/// The executable a process runs, if it can be queried.
fn process_image(pid: u32) -> Option<String> {
    // SAFETY: The handle is closed before returning; the buffer has the
    // declared length.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let read = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        read.then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

/// How far a window's top edge (`visible`) must drop for its title bar to
/// clear `strip`, the screen strip the menu bar keeps for itself; `None` when
/// it is not under it.
fn drop_below(visible: RECT, strip: RECT) -> Option<i32> {
    let under = visible.top < strip.bottom
        && visible.bottom > strip.top
        && visible.left < strip.right
        && visible.right > strip.left;
    under.then_some(strip.bottom - visible.top)
}

/// Moves a window whose title bar sits under the menu bar's `strip` to just
/// below it, as macOS keeps windows under its menu bar. The bar's reservation
/// only places maximized windows and those Windows positions itself: an app
/// may still open one at the very top of the screen, such as Firefox popups
/// from `window.open()`, and the bar, always on top, then hid its title bar
/// and caption buttons (issue #53).
///
/// Leaves alone maximized and minimized windows, windows without a title bar
/// (full screen, borderless, popups of menus), and a window the user is
/// still dragging: it moves once they let go.
pub fn keep_below(hwnd: isize, strip: RECT) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GUI_INMOVESIZE, GUITHREADINFO, GetGUIThreadInfo, SWP_NOZORDER, WS_CAPTION, WS_CHILD,
    };
    let window = HWND(hwnd as *mut _);
    // SAFETY: Read-only queries on a window handle; on a window that is gone
    // they fail and nothing moves.
    unsafe {
        if !IsWindowVisible(window).as_bool()
            || IsIconic(window).as_bool()
            || IsZoomed(window).as_bool()
            || is_cloaked(window)
        {
            return;
        }
        let style = GetWindowLongW(window, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 != WS_CAPTION.0 || style & WS_CHILD.0 != 0 {
            return;
        }
        let mut gui = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let thread = GetWindowThreadProcessId(window, None);
        if GetGUIThreadInfo(thread, &mut gui).is_ok() && gui.flags.contains(GUI_INMOVESIZE) {
            return;
        }
    }
    let Some(visible) = visible_rect(hwnd) else {
        return;
    };
    let (Some(drop), Some((whole, _))) = (drop_below(visible, strip), screen_rect(hwnd)) else {
        return;
    };
    // SAFETY: Moves another app's window without activating it or changing
    // its size or z-order; asynchronous, so a hung app cannot block the dock.
    unsafe {
        let _ = SetWindowPos(
            window,
            None,
            whole.left,
            whole.top + drop,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Where a window that has just started minimizing was on screen, which by
/// then it no longer says itself: it is already at -32000.
///
/// Returns the whole window, invisible resize borders included (where its
/// live thumbnail goes), and the part that was visible on screen (what the
/// genie copies and flies). `last_seen` is the [`screen_rect`] taken while
/// it was not maximized, if any.
///
/// A maximized window fills its monitor's work area, plus its borders: its
/// "normal" rectangle is the smaller one it returns to when un-maximized,
/// and flying from it made the genie jump first.
#[must_use]
pub fn rect_before_minimize(hwnd: isize, last_seen: Option<RECT>) -> Option<(RECT, RECT)> {
    use windows::Win32::{
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
    };
    let window = HWND(hwnd as *mut _);
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: The structure has its size initialized and lives for the call.
    unsafe { GetWindowPlacement(window, &mut placement) }.ok()?;
    if placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 == 0 {
        let rect = last_seen.or_else(|| normal_rect(hwnd))?;
        return Some((rect, rect));
    }
    // SAFETY: Read-only monitor, DPI and style queries.
    unsafe {
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let work = info.rcWork;
        // Windows maximizes a resizable window with its borders past the
        // work area's edges, so that only the window itself shows.
        let resizable = GetWindowLongW(window, GWL_STYLE) as u32 & WS_THICKFRAME.0 != 0;
        let border = if resizable {
            let dpi = GetDpiForWindow(window);
            GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        } else {
            0
        };
        let whole = RECT {
            left: work.left - border,
            top: work.top - border,
            right: work.right + border,
            bottom: work.bottom + border,
        };
        Some((whole, work))
    }
}

#[must_use]
pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// Recovers from a maximized Edge or Brave window covering the selected app
/// after focus has already moved away from the browser.
pub fn recover_browser_z_order(windows: &[TrackedWindow], foreground: isize) {
    recover_browser_z_order_with(windows, foreground, foreground_window);
}

fn recover_browser_z_order_with(
    windows: &[TrackedWindow],
    foreground: isize,
    read_foreground: impl Fn() -> isize,
) {
    let Some(active) = windows.iter().find(|window| window.hwnd == foreground) else {
        return;
    };
    let topmost = |hwnd: isize| {
        // SAFETY: Read-only query. Closed windows fail the geometry check.
        unsafe { GetWindowLongW(HWND(hwnd as *mut _), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
    };
    let Some((active_rect, _)) = screen_rect(foreground).filter(|_| !topmost(foreground)) else {
        return;
    };
    for window in windows {
        let browser = window.hwnd;
        if window.app_id == active.app_id
            || is_cloaked(HWND(browser as *mut _))
            || !matches!(
                window.app_id.rsplit('\\').next(),
                Some("msedge.exe" | "brave.exe")
            )
            || !topmost(browser)
        {
            continue;
        }
        let hwnd = HWND(browser as *mut _);
        // SAFETY: These queries only inspect the tracked browser window.
        let unowned = unsafe { GetWindow(hwnd, GW_OWNER).is_err() };
        let Some((browser_rect, true)) = screen_rect(browser).filter(|_| unowned) else {
            continue;
        };
        if !covers(browser_rect, active_rect) {
            continue;
        }

        // Place it below the last covered peer too, so a split terminal
        // group does not end up with just the focused half visible.
        let after = windows
            .iter()
            .rev()
            .filter(|peer| peer.app_id == active.app_id && !topmost(peer.hwnd))
            .find(|peer| screen_rect(peer.hwnd).is_some_and(|(rect, _)| covers(browser_rect, rect)))
            .map_or(foreground, |peer| peer.hwnd);
        if read_foreground() != foreground {
            return;
        }
        if topmost(after) || !topmost(browser) {
            continue;
        }
        // A position after a normal window also clears the topmost flag.
        // SAFETY: Queues position changes without moving, sizing or activating either window.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND(after as *mut _)),
                0,
                0,
                0,
                0,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
            );
        }
    }
}

/// Returns `true` if the foreground window covers the entire `screen`
/// (for example, video, presentation, or game). The desktop does not count,
/// and neither does a normally maximized window: its content still starts
/// below the title bar. Custom-frame apps may retain maximize/resize style
/// bits in full screen, so measure their content instead of those styles.
#[must_use]
pub fn foreground_is_fullscreen(screen: RECT) -> bool {
    // SAFETY: Read-only queries on the foreground window.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || is_desktop(hwnd) || is_task_switcher(hwnd) {
            return false;
        }
        // A window that is not on screen covers nothing. Right after signing
        // in, the foreground is often a shell host (Start, Search, the text
        // input host) whose screen-sized window is hidden or cloaked; taken
        // for a full-screen app, it kept the dock and the bar hidden.
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || is_cloaked(hwnd) {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return false;
        }
        fullscreen_owner_chain(hwnd, screen)
    }
}

fn fullscreen_owner_chain(mut hwnd: HWND, screen: RECT) -> bool {
    // A player's floating playback controls can take focus while their owner
    // stays full screen. Follow only that owner chain, never unrelated windows.
    // SAFETY: Read-only queries; bound traversal even if a window changes owners.
    unsafe {
        for _ in 0..16 {
            if IsWindowVisible(hwnd).as_bool()
                && !IsIconic(hwnd).as_bool()
                && !is_cloaked(hwnd)
                && fills_screen(hwnd, screen)
            {
                return true;
            }
            let Ok(owner) = GetWindow(hwnd, GW_OWNER) else {
                break;
            };
            if owner.is_invalid() {
                break;
            }
            hwnd = owner;
        }
    }
    false
}

fn fills_screen(hwnd: HWND, screen: RECT) -> bool {
    // SAFETY: Read-only queries with stack outputs; none sends a synchronous
    // message to the app, so a hung player cannot block the dock here.
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() || !covers(rect, screen) {
            return false;
        }
        // Browsers can draw their title bar inside the client area. When
        // normally maximized they still overflow the screen by the invisible
        // resize border; full-screen content fits the monitor itself.
        if GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_MAXIMIZE.0 != 0 && rect != screen {
            return false;
        }
        if GetClientRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let mut top_left = POINT {
            x: rect.left,
            y: rect.top,
        };
        let mut bottom_right = POINT {
            x: rect.right,
            y: rect.bottom,
        };
        ClientToScreen(hwnd, &mut top_left).as_bool()
            && ClientToScreen(hwnd, &mut bottom_right).as_bool()
            && covers(
                RECT {
                    left: top_left.x,
                    top: top_left.y,
                    right: bottom_right.x,
                    bottom: bottom_right.y,
                },
                screen,
            )
    }
}

/// `true` if `point` is on the minimize, maximize or close button of the
/// window under it. `screen` is the monitor the point is on.
pub fn caption_button_at(point: POINT, screen: RECT) -> bool {
    use windows::Win32::{
        Foundation::WPARAM,
        Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute},
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                GA_ROOT, GetAncestor, GetWindowRect, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
                WM_NCHITTEST, WindowFromPoint,
            },
        },
    };
    const HTMINBUTTON: usize = 8;
    const HTMAXBUTTON: usize = 9;
    const HTCLOSE: usize = 20;
    const HTHELP: usize = 21;
    /// Windows' caption buttons are 46 logical pixels wide, three of them.
    const BUTTONS_WIDTH: i32 = 3 * 46;
    // SAFETY: Read-only queries; the message has a short timeout and is
    // abandoned if the window does not answer.
    unsafe {
        let window = GetAncestor(WindowFromPoint(point), GA_ROOT);
        let mut frame = RECT::default();
        if window.is_invalid() || is_desktop(window) || GetWindowRect(window, &mut frame).is_err() {
            return false;
        }
        // A maximized window's frame overflows the screen by its border, so
        // its buttons reach the screen edge: the corner pixel is "close".
        let visible_right = frame.right.min(screen.right);
        let (x, y) = (point.x - frame.left, point.y - frame.top);

        // The buttons DWM draws for standard title bars.
        let mut buttons = RECT::default();
        if DwmGetWindowAttribute(
            window,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            (&raw mut buttons).cast(),
            size_of::<RECT>() as u32,
        )
        .is_ok()
            && buttons.right > buttons.left
        {
            let right = buttons.right.min(visible_right - frame.left);
            if x >= buttons.left && x <= right && y < buttons.bottom.max(1) {
                return true;
            }
        }
        // Apps that draw their own title bar (browsers, Office) answer with
        // the same hit-test codes, which Windows needs for Snap layouts.
        let position = ((point.y as u32 & 0xFFFF) << 16 | (point.x as u32 & 0xFFFF)) as isize;
        let mut hit = 0usize;
        let answered = SendMessageTimeoutW(
            window,
            WM_NCHITTEST,
            WPARAM(0),
            LPARAM(position),
            SMTO_ABORTIFHUNG,
            50,
            Some(&mut hit),
        );
        if answered.0 != 0 && matches!(hit, HTMINBUTTON | HTMAXBUTTON | HTCLOSE | HTHELP) {
            return true;
        }
        // Some draw them in web content and answer "client area" (Electron
        // apps). Every Windows app keeps them in the top-right corner, so a
        // window touching the top edge keeps that corner free of the bar.
        let width = BUTTONS_WIDTH * GetDpiForWindow(window).max(96) as i32 / 96;
        frame.top >= screen.top - 16 && point.x >= visible_right - width
    }
}

/// The Alt+Tab switcher (and, on Windows 11, Task View and Snap Assist,
/// which share its window class). It covers the screen but is not a
/// full-screen app.
pub fn is_task_switcher(hwnd: HWND) -> bool {
    matches!(
        class_name(hwnd).as_str(),
        "XamlExplorerHostIslandWindow" | "MultitaskingViewFrame"
    )
}

/// A window that is briefly in front while Windows switches between two
/// others (Alt+Tab passes through it), never the user's choice.
pub fn is_switch_transient(hwnd: HWND) -> bool {
    is_task_switcher(hwnd) || class_name(hwnd) == "ForegroundStaging"
}

/// The task switcher in front: `Some(true)` once on screen, `Some(false)`
/// while it is still appearing (it comes to the front some 50 ms before it
/// shows) or after it closed, `None` when another window is in front.
pub fn task_switcher_state() -> Option<bool> {
    // SAFETY: Read-only queries on the foreground window.
    unsafe {
        let front = GetForegroundWindow();
        is_task_switcher(front).then(|| IsWindowVisible(front).as_bool())
    }
}

/// Hidden by the system although "visible": on another virtual desktop, or
/// a suspended Store app.
pub(crate) fn is_cloaked(hwnd: HWND) -> bool {
    cloak_state(hwnd) != 0
}

fn cloak_state(hwnd: HWND) -> u32 {
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` is a valid 4-byte output.
    unsafe {
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
    }
    cloaked
}

fn workspace_cloak_allowed(cloaked: u32, show_other_workspaces: bool) -> bool {
    // Do not expose suspended Store apps or owner-cloaked helper windows,
    // including mixed flags. Windows desktops and tiling WMs share SHELL.
    cloaked == 0 || (show_other_workspaces && cloaked == DWM_CLOAKED_SHELL)
}

/// The desktop and the (hidden) taskbar: shell windows, not apps.
pub fn is_desktop(hwnd: HWND) -> bool {
    matches!(
        class_name(hwnd).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd"
    )
}

fn covers(window: RECT, screen: RECT) -> bool {
    window.left <= screen.left
        && window.top <= screen.top
        && window.right >= screen.right
        && window.bottom >= screen.bottom
}

fn is_app_window(hwnd: HWND, show_other_workspaces: bool) -> bool {
    // SAFETY: Read-only queries; calls on a destroyed `hwnd` have no effect.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let has =
            |flag: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE| style & flag.0 != 0;
        if has(WS_EX_TOOLWINDOW) {
            return false;
        }
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid());
        if (owned || has(WS_EX_NOACTIVATE)) && !has(WS_EX_APPWINDOW) {
            return false;
        }
        if !workspace_cloak_allowed(cloak_state(hwnd), show_other_workspaces) {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != GetCurrentProcessId()
    }
}

fn app_id(hwnd: HWND) -> Option<String> {
    if class_name(hwnd) == FRAME_HOST_CLASS {
        return window_aumid(hwnd).map(|aumid| format!("{APPS_FOLDER}{aumid}"));
    }

    // SAFETY: The process handle is closed before leaving the block, and the
    // buffers have the declared lengths.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let id = if GetApplicationUserModelId(process, &mut len, Some(PWSTR(buffer.as_mut_ptr())))
            .is_ok()
        {
            // `len` includes the null terminator.
            let aumid = String::from_utf16_lossy(&buffer[..len.saturating_sub(1) as usize]);
            Some(format!("{APPS_FOLDER}{aumid}"))
        } else {
            len = buffer.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
            .ok()
            .map(|()| {
                let image = String::from_utf16_lossy(&buffer[..len as usize]);
                // A program whose windows belong to a host that is no .exe
                // (LibreOffice's soffice.bin) has no icon or name of its own
                // there; its windows say which app they are (Calc, Writer),
                // and the taskbar shows that app.
                if is_program(&image) {
                    // A program's window may also belong to another app it
                    // runs: a web app in a browser, which the taskbar shows
                    // on its own.
                    window_aumid(hwnd)
                        .filter(|aumid| crate::pins::is_separate_app(aumid, &image))
                        .map_or_else(
                            || normalize_id(&image),
                            |aumid| format!("{APPS_FOLDER}{aumid}"),
                        )
                } else {
                    window_aumid(hwnd).map_or_else(
                        || normalize_id(&image),
                        |aumid| format!("{APPS_FOLDER}{aumid}"),
                    )
                }
            })
        };
        let _ = CloseHandle(process);
        id
    }
}

/// `true` for the path of a Windows shortcut (`.lnk`).
#[must_use]
pub fn is_shortcut(id: &str) -> bool {
    id.len() > 4 && id[id.len() - 4..].eq_ignore_ascii_case(".lnk")
}

/// `true` for the path of a program (`.exe`).
#[must_use]
pub fn is_program(path: &str) -> bool {
    path.len() > 4
        && path
            .get(path.len() - 4..)
            .is_some_and(|extension| extension.eq_ignore_ascii_case(".exe"))
}

/// The class of the window in front, to tell in the log what was there.
pub fn foreground_class() -> String {
    // SAFETY: Reads the foreground window handle.
    class_name(unsafe { GetForegroundWindow() })
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    // SAFETY: The buffer remains alive during the call, which returns the number of characters written.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn window_aumid(hwnd: HWND) -> Option<String> {
    // SAFETY: COM calls use pointers that remain valid for the duration of each call.
    let value = unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        store.GetValue(&PKEY_AppUserModel_ID).ok()?
    };
    let aumid = value.to_string();
    (!aumid.is_empty()).then_some(aumid)
}

/// Requests a window to close gracefully by posting `WM_CLOSE`.
/// Returns `true` if the message was successfully posted.
///
/// Never terminates the process; the application can prompt the user to save changes.
pub fn request_close_window(hwnd: isize) -> bool {
    let hwnd = HWND(hwnd as *mut _);
    // SAFETY: Read-only window validation and posting WM_CLOSE message.
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_program_is_found_by_its_full_path() -> std::io::Result<()> {
        let this = normalize_id(&std::env::current_exe()?.to_string_lossy());
        assert!(is_running(&this));
        // Same file name, another folder: not the same program.
        let elsewhere = format!(
            "c:\\no-such-folder\\{}",
            this.rsplit('\\').next().unwrap_or_default()
        );
        assert!(!is_running(&elsewhere));
        assert!(!is_running(
            "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        ));
        Ok(())
    }

    #[test]
    fn only_windows_under_the_menu_bar_drop_below_it() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let bar = rect(0, 0, 1920, 35);
        // A popup opened at the top of the screen (issue #53).
        assert_eq!(drop_below(rect(300, 0, 900, 700), bar), Some(35));
        // Partly under it, or above the screen.
        assert_eq!(drop_below(rect(300, 20, 900, 700), bar), Some(15));
        assert_eq!(drop_below(rect(300, -40, 900, 700), bar), Some(75));
        // Already below the bar, touching it, or on another monitor.
        assert_eq!(drop_below(rect(300, 35, 900, 700), bar), None);
        assert_eq!(drop_below(rect(300, 200, 900, 700), bar), None);
        assert_eq!(drop_below(rect(2000, 0, 2600, 700), bar), None);
    }

    #[test]
    fn workspace_cloaking_is_opt_in_without_exposing_app_or_inherited_cloaks() {
        use windows::Win32::Graphics::Dwm::{DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED};

        assert!(workspace_cloak_allowed(0, false));
        assert!(workspace_cloak_allowed(0, true));
        assert!(!workspace_cloak_allowed(DWM_CLOAKED_SHELL, false));
        assert!(workspace_cloak_allowed(DWM_CLOAKED_SHELL, true));
        for flags in [
            DWM_CLOAKED_APP,
            DWM_CLOAKED_INHERITED,
            DWM_CLOAKED_SHELL | DWM_CLOAKED_APP,
            DWM_CLOAKED_SHELL | DWM_CLOAKED_INHERITED,
            8,
        ] {
            assert!(!workspace_cloak_allowed(flags, false), "flags={flags}");
            assert!(!workspace_cloak_allowed(flags, true), "flags={flags}");
        }
    }

    #[test]
    fn fullscreen_geometry_should_follow_content_instead_of_maximized_style()
    -> windows::core::Result<()> {
        use windows::{
            Win32::{
                Foundation::LRESULT,
                System::LibraryLoader::GetModuleHandleW,
                UI::WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
                    SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SetWindowLongW, ShowWindow,
                    UnregisterClassW, WM_NCCALCSIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_POPUP,
                },
            },
            core::w,
        };

        // Qt and other custom-frame apps can retain resize/maximize styles
        // while returning the whole window as client area in full-screen mode.
        unsafe extern "system" fn frameless(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if msg == WM_NCCALCSIZE {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, w, l) }
            }
        }
        struct Fixture(HWND);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let class = w!("NosihFullscreenGeometryTest");
        // Small virtual displays avoid Windows' maximum tracking-size limit
        // on test machines with smaller monitors or different DPI settings.
        const SCREEN: RECT = RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        // The test's windows stay hidden, never taking focus from the user.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let registered = RegisterClassW(&WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: class,
                lpfnWndProc: Some(frameless),
                ..Default::default()
            });
            assert_ne!(registered, 0);
            let run = || -> windows::core::Result<()> {
                for screen in [
                    SCREEN,
                    RECT {
                        left: -800,
                        right: 0,
                        ..SCREEN
                    },
                ] {
                    for style in [WS_POPUP, WS_POPUP | WS_THICKFRAME | WS_MAXIMIZE] {
                        let window = Fixture(CreateWindowExW(
                            Default::default(),
                            class,
                            w!("Fullscreen fixture"),
                            style & !WS_MAXIMIZE,
                            screen.left,
                            screen.top,
                            800,
                            600,
                            None,
                            None,
                            Some(instance.into()),
                            None,
                        )?);
                        SetWindowPos(
                            window.0,
                            None,
                            screen.left,
                            screen.top,
                            800,
                            600,
                            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                        )?;
                        // Preserve the requested full-screen geometry rather than
                        // letting Windows maximize it to this machine's work area.
                        SetWindowLongW(window.0, GWL_STYLE, style.0 as i32);
                        let mut actual = RECT::default();
                        let mut content = RECT::default();
                        let mut origin = POINT::default();
                        GetWindowRect(window.0, &mut actual)?;
                        GetClientRect(window.0, &mut content)?;
                        let _ = ClientToScreen(window.0, &mut origin);
                        assert!(
                            fills_screen(window.0, screen),
                            "full-screen content, style {style:?}, window {actual:?}, client {content:?}, origin {origin:?}, screen {screen:?}"
                        );
                        assert!(
                            !fills_screen(
                                window.0,
                                RECT {
                                    left: screen.right,
                                    right: screen.right + 800,
                                    ..screen
                                }
                            ),
                            "a different display must stay usable"
                        );
                        SetWindowLongW(window.0, GWL_STYLE, (style & !WS_MAXIMIZE).0 as i32);
                        SetWindowPos(
                            window.0,
                            None,
                            screen.left,
                            screen.top,
                            400,
                            300,
                            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                        )?;
                        SetWindowLongW(window.0, GWL_STYLE, style.0 as i32);
                        assert!(
                            !fills_screen(window.0, screen),
                            "leaving full screen must restore the dock"
                        );
                        if style.0 & WS_MAXIMIZE.0 != 0 {
                            SetWindowLongW(window.0, GWL_STYLE, (style & !WS_MAXIMIZE).0 as i32);
                            SetWindowPos(
                                window.0,
                                None,
                                screen.left - 8,
                                screen.top - 8,
                                816,
                                616,
                                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                            )?;
                            SetWindowLongW(window.0, GWL_STYLE, style.0 as i32);
                            assert!(
                                !fills_screen(window.0, screen),
                                "a normally maximized custom-frame browser must keep the dock available"
                            );
                        }
                    }
                    let captioned = Fixture(CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("Captioned fixture"),
                        WS_OVERLAPPEDWINDOW,
                        screen.left,
                        screen.top,
                        800,
                        600,
                        None,
                        None,
                        Some(instance.into()),
                        None,
                    )?);
                    assert!(
                        !fills_screen(captioned.0, screen),
                        "a captioned screen-sized window is not full screen"
                    );
                }
                // Off-screen, non-activating fixtures exercise actual Win32
                // ownership and visibility without covering the user's apps.
                let screen = RECT {
                    left: -16000,
                    top: -16000,
                    right: -15200,
                    bottom: -15400,
                };
                let owner = Fixture(CreateWindowExW(
                    WS_EX_NOACTIVATE,
                    class,
                    w!("Fullscreen owner"),
                    WS_POPUP,
                    screen.left,
                    screen.top,
                    800,
                    600,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )?);
                let controls = Fixture(CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class,
                    w!("Playback controls"),
                    WS_POPUP,
                    screen.left,
                    screen.bottom - 50,
                    300,
                    50,
                    Some(owner.0),
                    None,
                    Some(instance.into()),
                    None,
                )?);
                let _ = ShowWindow(owner.0, SW_SHOWNOACTIVATE);
                let _ = ShowWindow(controls.0, SW_SHOWNOACTIVATE);
                assert!(
                    fullscreen_owner_chain(controls.0, screen),
                    "playback controls must preserve their owner's fullscreen state"
                );
                SetWindowPos(
                    owner.0,
                    None,
                    screen.left,
                    screen.top,
                    400,
                    300,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                )?;
                assert!(
                    !fullscreen_owner_chain(controls.0, screen),
                    "leaving fullscreen must clear the owned controls too"
                );
                Ok(())
            };
            let result = run();
            let _ = UnregisterClassW(class, Some(instance.into()));
            result
        }
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop; only moves test windows"]
    fn blocking_browser_should_move_behind_both_windows_of_the_selected_app()
    -> windows::core::Result<()> {
        use windows::{
            Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GW_HWNDNEXT, HWND_TOP, SW_SHOWNOACTIVATE,
                SetWindowLongW, ShowWindow, WS_EX_TOPMOST, WS_POPUP,
            },
            core::w,
        };

        struct Fixture(HWND);
        impl Drop for Fixture {
            fn drop(&mut self) {
                // SAFETY: The test owns this window for its entire lifetime.
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let create = |x, width, browser: bool| unsafe {
            let ex = if browser {
                WS_EX_NOACTIVATE | WS_EX_TOPMOST
            } else {
                WS_EX_NOACTIVATE
            };
            let window = Fixture(CreateWindowExW(
                ex,
                w!("STATIC"),
                w!("Nosih browser ordering test"),
                WS_POPUP,
                x,
                100,
                width,
                160,
                None,
                None,
                None,
                None,
            )?);
            let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
            if browser {
                // Mark the fixture maximized without activating it or moving
                // it out of the test's controlled geometry.
                SetWindowLongW(window.0, GWL_STYLE, (WS_POPUP | WS_MAXIMIZE).0 as i32);
            }
            windows::core::Result::Ok(window)
        };
        let left = create(100, 180, false)?;
        let right = create(280, 180, false)?;
        let browser = create(80, 400, true)?;
        let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
        unsafe {
            SetWindowPos(right.0, Some(HWND_TOP), 0, 0, 0, 0, flags)?;
            assert!(IsZoomed(browser.0).as_bool());
            assert!(GetWindowLongW(browser.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0);
        }
        let windows = [
            (&browser, "c:\\browser\\msedge.exe"),
            (&right, "terminal"),
            (&left, "terminal"),
        ]
        .map(|(window, id)| TrackedWindow {
            hwnd: window.0.0 as isize,
            app_id: id.into(),
            minimized: false,
        });
        let foreground = right.0.0 as isize;
        recover_browser_z_order_with(&windows, foreground, || left.0.0 as isize);
        assert!(unsafe { GetWindowLongW(browser.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 });
        recover_browser_z_order_with(&windows, foreground, || foreground);
        assert_eq!(unsafe { GetWindow(left.0, GW_HWNDNEXT)? }, browser.0);
        assert_eq!(
            unsafe { GetWindowLongW(browser.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 },
            0
        );
        Ok(())
    }

    const SCREEN: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn covers_should_accept_window_exactly_the_size_of_the_screen() {
        assert!(covers(SCREEN, SCREEN));
    }

    #[test]
    fn covers_should_reject_maximized_window_above_the_dock() {
        // A maximized window extends a few pixels beyond the sides and top, but
        // ends above the strip reserved by the dock.
        let maximized = RECT {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 985,
        };
        assert!(!covers(maximized, SCREEN));
    }

    #[test]
    fn normalize_id_should_lowercase_executable_paths() {
        assert_eq!(
            normalize_id("C:\\Windows\\Explorer.EXE"),
            "c:\\windows\\explorer.exe"
        );
    }

    #[test]
    fn is_program_should_accept_only_exe_files() {
        assert!(is_program(r"C:\Program Files\App\App.EXE"));
        assert!(!is_program(
            r"C:\Program Files\LibreOffice\program\soffice.bin"
        ));
        assert!(!is_program(".exe"));
        // A name ending inside a multi-byte character is no program, never a panic.
        assert!(!is_program(r"C:\app\x€ab"));
    }

    #[test]
    fn normalize_id_should_keep_packaged_app_ids_untouched() {
        let id = "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";
        assert_eq!(normalize_id(id), id);
    }
}
