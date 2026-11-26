//! Where apps send their taskbar buttons' overlay icons (issue #65): the
//! red dot or unread count Slack, Zoom or Teams put on their button.
//!
//! Windows has no way to read another app's overlay. But an app does not
//! talk to the taskbar directly: its `ITaskbarList` finds the window to send
//! each call to in the `TaskbandHWND` property of the taskbar
//! (`Shell_TrayWnd`), normally Explorer's own task band (`MSTaskSwWClass`).
//! Pointing that property at a window of ours brings us the calls, as
//! ManagedShell-based taskbars (RetroBar, Cairo) do with Explorer running.
//!
//! The taskbar is hidden while the dock runs, so Explorer missing those
//! calls shows nowhere; the property goes back to Explorer when the dock
//! stops, and the watchdog gives it back if the dock dies.
//!
//! An app reads the property when it creates its `ITaskbarList`: apps
//! started after the dock reach it, one already running may keep talking to
//! Explorer until it is restarted.

use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumChildWindows, FindWindowW, GetClassNameW, GetPropW, IsWindow, SetPropW, WM_USER,
        },
    },
    core::{BOOL, PCWSTR, w},
};

const PROPERTY: PCWSTR = w!("TaskbandHWND");
/// The class of the window that takes the calls, so the watchdog can tell
/// a property left behind by a dock that died.
pub const WINDOW_CLASS: PCWSTR = w!("NosihTaskband");

/// `ITaskbarList3::SetOverlayIcon`: the button's window in `wParam`, the icon
/// in `lParam` (null to remove it). The caller may destroy the icon once the
/// message returns: copy it before.
pub const SET_OVERLAY_ICON: u32 = WM_USER + 79;
/// Every `ITaskbarList` call arrives in this range of messages (progress,
/// thumbnail buttons, tabs...); answered with success, as the hidden
/// taskbar would show none of them.
pub const CALLS: std::ops::RangeInclusive<u32> = (WM_USER + 50)..=(WM_USER + 100);

fn tray() -> Option<HWND> {
    // SAFETY: Read-only lookup with a static class name.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok()
}

/// Explorer's task band, where the property points when nobody took it.
fn explorer_band(tray: HWND) -> Option<HWND> {
    unsafe extern "system" fn find(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 32];
        // SAFETY: `class` is a valid buffer; `found` points to the Option
        // below, alive for the whole enumeration.
        unsafe {
            let len = GetClassNameW(hwnd, &mut class);
            if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "MSTaskSwWClass" {
                *(found.0 as *mut Option<HWND>) = Some(hwnd);
                return false.into();
            }
        }
        true.into()
    }
    let mut found: Option<HWND> = None;
    // SAFETY: The callback only writes to `found`, which outlives the call.
    unsafe {
        let _ = EnumChildWindows(Some(tray), Some(find), LPARAM(&raw mut found as isize));
    }
    found
}

/// Points the taskbar's property at `window`: from now on, apps that create
/// an `ITaskbarList` send their calls there. `false` without a taskbar.
pub fn claim(window: HWND) -> bool {
    let Some(tray) = tray() else {
        return false;
    };
    // SAFETY: A property on Explorer's window, holding a window handle.
    unsafe { SetPropW(tray, PROPERTY, Some(HANDLE(window.0))) }.is_ok()
}

/// Gives the property back to Explorer if `window` still holds it.
pub fn release(window: HWND) {
    let Some(tray) = tray() else {
        return;
    };
    // SAFETY: Read-only property lookup.
    if unsafe { GetPropW(tray, PROPERTY) }.0 == window.0 {
        give_back(tray);
    }
}

/// For the watchdog: gives the property back to Explorer if a dock left it
/// pointing at a window that is gone or is ours; another taskbar replacement
/// holding it is left alone.
pub fn restore_after_exit() {
    let Some(tray) = tray() else {
        return;
    };
    // SAFETY: Read-only lookups on window handles.
    let held = unsafe { GetPropW(tray, PROPERTY) };
    let holder = HWND(held.0);
    let stale = held.is_invalid() || !unsafe { IsWindow(Some(holder)) }.as_bool() || {
        let mut class = [0u16; 32];
        // SAFETY: `class` is a valid buffer.
        let len = unsafe { GetClassNameW(holder, &mut class) };
        // SAFETY: A static, null-terminated class name.
        let ours = unsafe { WINDOW_CLASS.to_string() }.unwrap_or_default();
        String::from_utf16_lossy(&class[..len.max(0) as usize]) == ours
    };
    if stale {
        give_back(tray);
    }
}

fn give_back(tray: HWND) {
    if let Some(band) = explorer_band(tray) {
        // SAFETY: Explorer's own task band, as Windows set it.
        let _ = unsafe { SetPropW(tray, PROPERTY, Some(HANDLE(band.0))) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_cover_the_overlay_icon() {
        assert!(CALLS.contains(&SET_OVERLAY_ICON));
        assert_eq!(SET_OVERLAY_ICON, 0x400 + 79);
    }

    #[test]
    #[ignore = "Points the live taskbar's TaskbandHWND at a test window for a moment"]
    fn claiming_and_releasing_gives_the_taskbar_back_to_explorer() -> windows::core::Result<()> {
        let Some(tray) = tray() else {
            return Ok(());
        };
        // SAFETY: Read-only lookups and our own hidden window.
        unsafe {
            let before = GetPropW(tray, PROPERTY);
            let window = windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("taskband test"),
                Default::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )?;
            assert!(claim(window));
            assert_eq!(GetPropW(tray, PROPERTY).0, window.0);
            release(window);
            assert_eq!(GetPropW(tray, PROPERTY).0, before.0);
            // Left behind by a dock that died: given back.
            assert!(claim(window));
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(window);
            restore_after_exit();
            assert_eq!(GetPropW(tray, PROPERTY).0, before.0);
        }
        Ok(())
    }
}
