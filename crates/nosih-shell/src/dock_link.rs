//! How other Nosih processes talk to the running dock.

use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            FindWindowW, PostMessageW, RegisterWindowMessageW, SPI_SETDESKWALLPAPER,
            WM_SETTINGCHANGE,
        },
    },
    core::{PCWSTR, w},
};

pub const DOCK_WINDOW_CLASS: PCWSTR = w!("NosihWindow");
/// Name of the registered window message meaning "the configuration file changed".
pub const CONFIG_CHANGED: PCWSTR = w!("Nosih.ConfigChanged");
/// `true` if a dock is running in this session.
#[must_use]
pub fn is_running() -> bool {
    // SAFETY: Read-only window lookup.
    unsafe { FindWindowW(DOCK_WINDOW_CLASS, None).is_ok() }
}

/// Tells the dock, if it is running, to reload its configuration. A window
/// message instead of a file watcher: the dock spends nothing while idle.
pub fn notify_config_changed() {
    // SAFETY: Looks up a window and posts a message without associated data.
    unsafe {
        if let Ok(dock) = FindWindowW(DOCK_WINDOW_CLASS, None) {
            let message = RegisterWindowMessageW(CONFIG_CHANGED);
            let _ = PostMessageW(Some(dock), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// Refreshes Nosih's backdrop after a successful wallpaper change. Posting
/// only to our window neither waits for nor wakes unrelated applications.
pub fn notify_wallpaper_changed() {
    // SAFETY: No pointers or borrowed data cross the process boundary.
    unsafe {
        if let Ok(dock) = FindWindowW(DOCK_WINDOW_CLASS, None) {
            let _ = PostMessageW(
                Some(dock),
                WM_SETTINGCHANGE,
                WPARAM(SPI_SETDESKWALLPAPER.0 as usize),
                LPARAM(0),
            );
        }
    }
}
