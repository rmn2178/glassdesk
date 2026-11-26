//! Hides the Windows taskbar while the dock is active and restores its previous state.
//!
//! Auto-hide (which frees the work area) is combined with making the taskbar
//! window invisible and click-through. `ShowWindow(SW_HIDE)` does not work on
//! Windows 11 because Explorer shows the taskbar again.
//!
//! The original auto-hide setting is saved the first time the taskbar is taken
//! over and deleted only when it is restored. If the dock exits unexpectedly
//! and restarts, its own auto-hide setting is not mistaken for the user's.

use std::{env, fs, mem::size_of, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM},
        UI::{
            Shell::{ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage},
            WindowsAndMessaging::{
                FindWindowExW, FindWindowW, GWL_EXSTYLE, GetLayeredWindowAttributes,
                GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
                WS_EX_LAYERED, WS_EX_TRANSPARENT,
            },
        },
    },
    core::{PCWSTR, w},
};

const PRIMARY: PCWSTR = w!("Shell_TrayWnd");
const SECONDARY: PCWSTR = w!("Shell_SecondaryTrayWnd");
const GHOST_STYLES: isize = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;

fn state_file() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("Nosih")
        .join("taskbar-original")
}

/// Hides the taskbar on all monitors.
///
/// Idempotent; can be called again after Explorer restarts or Windows redraws
/// the taskbar.
pub fn take_over() -> bool {
    take_over_taskbars(&taskbars(), &state_file())
}

/// Whether Explorer has restored the visibility of a taskbar window.
pub fn needs_visibility_repair() -> bool {
    taskbars().into_iter().any(|hwnd| !is_ghosted(hwnd))
}

fn take_over_taskbars(bars: &[HWND], file: &std::path::Path) -> bool {
    // A logon task can run before Explorer creates Shell_TrayWnd. Saving
    // an absent shell's state would lose the user's original preference.
    if bars.is_empty() {
        return false;
    }
    if !file.exists() {
        let original = if autohide_enabled() { "1" } else { "0" };
        let saved = file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(file, original));
        if let Err(error) = saved {
            // Never hide the system taskbar unless the guard can restore its
            // original state after a crash or hang.
            crate::log::line(&format!(
                "no se pudo guardar el estado de la barra: {error}"
            ));
            return false;
        }
    }
    if !autohide_enabled() {
        set_autohide(true);
    }
    let mut hidden = true;
    for &hwnd in bars {
        hidden &= is_ghosted(hwnd) || set_ghost(hwnd, true);
    }
    hidden
}

/// What the dock and its watchdog do when the dock stops: give the taskbar
/// back, except while Windows shuts down, restarts or signs out with Nosih
/// set to start again. Then the taskbar stays auto-hidden for the next
/// sign-in; given back, Windows showed it in full at every start until the
/// dock took it over again. Turning autostart off, quitting or uninstalling
/// still gives it back.
pub fn give_back_on_exit() {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    // SAFETY: Read-only system query.
    let session_ending = unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } != 0;
    if session_ending && crate::autostart::is_enabled() {
        return;
    }
    give_back();
}

/// Restores the taskbar to its state before `take_over`. Has no effect if the
/// dock did not take it over.
pub fn give_back() {
    let file = state_file();
    let Ok(original) = fs::read_to_string(&file) else {
        // No saved state; make sure the taskbar is visible anyway.
        for hwnd in taskbars() {
            set_ghost(hwnd, false);
        }
        return;
    };
    for hwnd in taskbars() {
        set_ghost(hwnd, false);
    }
    set_autohide(original.trim() == "1");
    let _ = fs::remove_file(file);
}

/// Whether `hwnd` is a taskbar Nosih has hidden. Windows still gives it the
/// foreground when nothing else can take it (the last window closes, a
/// full-screen app ends), and its focused "^" button then shows its tooltip,
/// "Show hidden icons", in a popup of its own over the desktop (issue #48).
#[must_use]
pub fn is_hidden(hwnd: HWND) -> bool {
    is_taskbar(hwnd) && is_ghosted(hwnd)
}

/// Whether `hwnd` is the taskbar of one of the monitors, hidden or not.
#[must_use]
pub fn is_taskbar(hwnd: HWND) -> bool {
    taskbars().contains(&hwnd)
}

fn taskbars() -> Vec<HWND> {
    let mut found = Vec::new();
    // SAFETY: Read-only calls with valid static strings.
    unsafe {
        if let Ok(hwnd) = FindWindowW(PRIMARY, None) {
            found.push(hwnd);
        }
        let mut previous = None;
        while let Ok(hwnd) = FindWindowExW(None, previous, SECONDARY, None) {
            found.push(hwnd);
            previous = Some(hwnd);
        }
    }
    found
}

fn appbar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: taskbars().first().copied().unwrap_or_default(),
        ..Default::default()
    }
}

fn autohide_enabled() -> bool {
    let mut data = appbar_data();
    // SAFETY: `data` is a valid APPBARDATA with its size initialized.
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    state as u32 & ABS_AUTOHIDE != 0
}

fn set_autohide(enabled: bool) {
    let mut data = appbar_data();
    data.lParam = LPARAM(if enabled { ABS_AUTOHIDE as isize } else { 0 });
    // SAFETY: Same as above.
    unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
}

fn set_ghost(hwnd: HWND, ghost: bool) -> bool {
    // SAFETY: `hwnd` comes from FindWindow; if the window no longer exists, the calls have no effect.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ghost {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | GHOST_STYLES);
            if SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA).is_err() {
                return false;
            }
        } else if style & GHOST_STYLES != 0 {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !GHOST_STYLES);
        }
    }
    !ghost || is_ghosted(hwnd)
}

fn is_ghosted(hwnd: HWND) -> bool {
    // SAFETY: Read-only queries with valid outputs. A destroyed window fails the check.
    unsafe {
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & GHOST_STYLES != GHOST_STYLES {
            return false;
        }
        let mut alpha = 255;
        let mut flags = Default::default();
        GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)).is_ok()
            && alpha == 0
            && flags.contains(LWA_ALPHA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_shell_does_not_save_a_false_original_taskbar_state() {
        let file = env::temp_dir().join(format!("nosih-absent-shell-{}.txt", std::process::id()));
        assert!(!file.exists());
        assert!(!take_over_taskbars(&[], &file));
        assert!(!file.exists());
    }

    #[test]
    fn a_destroyed_taskbar_is_not_reported_as_successfully_hidden() {
        assert!(!set_ghost(HWND::default(), true));
    }

    #[test]
    fn ghost_visibility_is_verified_and_can_be_restored() -> windows::core::Result<()> {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};
        // A hidden test window, never the user's real taskbar.
        unsafe {
            let window = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Nosih taskbar test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?;
            let initially_ghosted = is_ghosted(window);
            let hidden = set_ghost(window, true);
            SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA)?;
            let reset_was_detected = !is_ghosted(window);
            let hidden_again = set_ghost(window, true);
            let restored = set_ghost(window, false);
            let styles = GetWindowLongPtrW(window, GWL_EXSTYLE);
            let _ = DestroyWindow(window);
            assert!(!initially_ghosted);
            assert!(hidden);
            assert!(reset_was_detected);
            assert!(hidden_again);
            assert!(restored);
            assert_eq!(styles & GHOST_STYLES, 0);
        }
        Ok(())
    }
}
