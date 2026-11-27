//! Safe, non-blocking Win32 window actions for the active foreground window.

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow,
            IsWindowVisible, IsZoomed, PostMessageW, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
            SW_SHOWNORMAL, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            SetWindowPlacement, SetWindowPos, ShowWindowAsync, WINDOWPLACEMENT, WM_CLOSE,
        },
    },
    core::PWSTR,
};

/// Minimizes `hwnd` asynchronously so a hung application cannot block the dock.
pub fn minimize(hwnd: isize) {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Queued window-state request; no-op if the handle is invalid or gone.
    unsafe {
        if !window.is_invalid() && IsWindow(Some(window)).as_bool() {
            let _ = ShowWindowAsync(window, SW_MINIMIZE);
        }
    }
}

/// Toggles `hwnd` between maximized and restored states asynchronously.
pub fn toggle_zoom(hwnd: isize) {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Read-only zoom check followed by an asynchronous show-state request.
    unsafe {
        if !window.is_invalid() && IsWindow(Some(window)).as_bool() {
            let cmd = if IsZoomed(window).as_bool() {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            };
            let _ = ShowWindowAsync(window, cmd);
        }
    }
}

/// Computes the top-left `(x, y)` origin to center a window of `size = (w, h)`
/// inside a monitor work area `work = (left, top, right, bottom)`.
///
/// Oversized windows are clamped so their top-left corner stays within the work area.
#[must_use]
pub fn centered_origin(work: (i32, i32, i32, i32), size: (i32, i32)) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let (width, height) = size;
    let work_w = (right - left).max(0);
    let work_h = (bottom - top).max(0);
    let win_w = width.max(0);
    let win_h = height.max(0);
    let x = (left + (work_w - win_w) / 2).max(left);
    let y = (top + (work_h - win_h) / 2).max(top);
    (x, y)
}

/// Centers `hwnd` on the work area of its current monitor without resizing or
/// changing z-order or focus.
pub fn center_on_monitor(hwnd: isize) {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Read-only geometry queries followed by an asynchronous move of a
    // live window; fails harmlessly if the window closes mid-call.
    unsafe {
        if window.is_invalid()
            || !IsWindow(Some(window)).as_bool()
            || IsIconic(window).as_bool()
        {
            return;
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let work = info.rcWork;
        if IsZoomed(window).as_bool() {
            let mut placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(window, &mut placement).is_err() {
                return;
            }
            let norm = placement.rcNormalPosition;
            let w = (norm.right - norm.left).max(320);
            let h = (norm.bottom - norm.top).max(240);
            let (x, y) = centered_origin((work.left, work.top, work.right, work.bottom), (w, h));
            placement.rcNormalPosition = RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            placement.showCmd = SW_SHOWNORMAL.0 as u32;
            let _ = SetWindowPlacement(window, &placement);
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_err() {
            return;
        }
        let (x, y) = centered_origin(
            (work.left, work.top, work.right, work.bottom),
            (rect.right - rect.left, rect.bottom - rect.top),
        );
        let _ = SetWindowPos(
            window,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Posts `WM_CLOSE` to `hwnd` so the application can prompt to save changes.
pub fn close_window(hwnd: isize) {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Asynchronous message post; no-op if the window no longer exists.
    unsafe {
        if !window.is_invalid() && IsWindow(Some(window)).as_bool() {
            let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Minimizes every valid, visible, non-minimized, non-cloaked window in
/// `tracked_hwnds` whose handle is not in `keep_hwnds`.
pub fn hide_others(keep_hwnds: &[isize], tracked_hwnds: &[isize]) {
    for &raw in tracked_hwnds {
        if keep_hwnds.contains(&raw) {
            continue;
        }
        let window = HWND(raw as *mut _);
        // SAFETY: Read-only visibility checks and asynchronous minimize.
        unsafe {
            if !window.is_invalid()
                && IsWindow(Some(window)).as_bool()
                && IsWindowVisible(window).as_bool()
                && !IsIconic(window).as_bool()
                && !crate::tracker::is_cloaked(window)
            {
                let _ = ShowWindowAsync(window, SW_MINIMIZE);
            }
        }
    }
}

/// Restores minimized windows in `hwnds` and brings the group to the front.
pub fn bring_all_to_front(hwnds: &[isize]) {
    for &raw in hwnds {
        let window = HWND(raw as *mut _);
        // SAFETY: Queued restore request on live, visible, minimized windows.
        unsafe {
            if !window.is_invalid()
                && IsWindow(Some(window)).as_bool()
                && IsWindowVisible(window).as_bool()
                && IsIconic(window).as_bool()
                && !crate::tracker::is_cloaked(window)
            {
                let _ = ShowWindowAsync(window, SW_RESTORE);
            }
        }
    }
    crate::launcher::activate(hwnds);
}

/// Resolves the executable path backing `hwnd` and launches a new instance via
/// `ShellExecuteExW` (`crate::launcher::open`).
pub fn open_new_window(hwnd: isize) {
    let Some(path) = executable_path(hwnd) else {
        return;
    };
    let _ = crate::launcher::open(&path);
}

fn executable_path(hwnd: isize) -> Option<String> {
    let window = HWND(hwnd as *mut _);
    // SAFETY: Read-only process query; the opened process handle is always closed.
    unsafe {
        if window.is_invalid() || !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        if !ok || len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_origin_centers_on_primary_monitor() {
        assert_eq!(centered_origin((0, 0, 1920, 1040), (800, 600)), (560, 220));
    }

    #[test]
    fn centered_origin_handles_offset_and_negative_monitors() {
        assert_eq!(
            centered_origin((-1920, 32, 0, 1080), (1000, 600)),
            (-1460, 256)
        );
        assert_eq!(
            centered_origin((1920, 0, 3840, 1080), (640, 480)),
            (2560, 300)
        );
    }

    #[test]
    fn centered_origin_clamps_oversized_window_to_work_origin() {
        assert_eq!(
            centered_origin((100, 40, 1380, 760), (2000, 1200)),
            (100, 40)
        );
    }
}
