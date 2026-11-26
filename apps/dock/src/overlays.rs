//! The overlay icons apps put on their taskbar buttons (issue #65), shown on
//! their dock icons: the red dot or unread count of Slack, Zoom or Teams.
//!
//! A hidden window of ours takes the calls apps make to the taskbar (see
//! `nosih_shell::taskband`). Each overlay is copied as it arrives, as the
//! app may destroy its icon once the call returns, and kept here, outside
//! the app state: calls from other apps arrive whenever this thread pumps
//! messages, also while that state is in use. The dock is told with a
//! posted message and reads the overlays when it draws its badges.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nosih_shell::{
    icons::{self, IconImage},
    taskband,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, HICON, PostMessageW, RegisterClassW,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSW,
    },
};

/// Posted to the dock's window when an overlay changed.
pub const OVERLAYS_CHANGED: u32 = WM_APP + 12;

#[derive(Default)]
struct State {
    /// Our window, while it holds the taskbar's calls.
    window: Option<HWND>,
    /// The dock's window, told of changes.
    notify: Option<HWND>,
    /// Each window's overlay, by window handle.
    images: HashMap<isize, Rc<IconImage>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Starts taking the taskbar's calls; `notify` hears of every change.
/// Does nothing if they are taken already.
pub fn start(notify: HWND) {
    if STATE.with_borrow(|state| state.window.is_some()) {
        return;
    }
    let Some(window) = create_window() else {
        return;
    };
    if !taskband::claim(window) {
        // SAFETY: Our own window, created above.
        unsafe {
            let _ = DestroyWindow(window);
        }
        return;
    }
    STATE.with_borrow_mut(|state| {
        state.window = Some(window);
        state.notify = Some(notify);
    });
}

/// Takes the calls again after Explorer restarted: its new taskbar points
/// the property at its own task band.
pub fn reclaim() {
    if let Some(window) = STATE.with_borrow(|state| state.window) {
        taskband::claim(window);
    }
}

/// Gives the taskbar's calls back to Explorer and forgets every overlay.
pub fn stop() {
    let window = STATE.with_borrow_mut(|state| {
        state.images.clear();
        state.window.take()
    });
    if let Some(window) = window {
        taskband::release(window);
        // SAFETY: Our own window.
        unsafe {
            let _ = DestroyWindow(window);
        }
    }
}

/// The overlay of the first of `windows` that has one.
pub fn image_for(mut windows: impl Iterator<Item = isize>) -> Option<Rc<IconImage>> {
    STATE.with_borrow(|state| windows.find_map(|hwnd| state.images.get(&hwnd).cloned()))
}

/// Drops the overlays of windows that are gone (handles are reused).
pub fn retain(alive: impl Fn(isize) -> bool) {
    STATE.with_borrow_mut(|state| state.images.retain(|&hwnd, _| alive(hwnd)));
}

fn set(hwnd: isize, image: Option<IconImage>) {
    let notify = STATE.with_borrow_mut(|state| {
        match image {
            Some(image) => {
                state.images.insert(hwnd, Rc::new(image));
            }
            None => {
                state.images.remove(&hwnd)?;
            }
        }
        state.notify
    });
    if let Some(notify) = notify {
        // SAFETY: Only a payload-free notification to our own window.
        unsafe {
            let _ = PostMessageW(Some(notify), OVERLAYS_CHANGED, WPARAM(0), LPARAM(0));
        }
    }
}

fn create_window() -> Option<HWND> {
    static CLASS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    // SAFETY: The class is registered once; the window is destroyed by
    // `stop`. Never shown: it only receives messages.
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        CLASS.get_or_init(|| {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(taskband_proc),
                hInstance: instance.into(),
                lpszClassName: taskband::WINDOW_CLASS,
                ..Default::default()
            });
        });
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            taskband::WINDOW_CLASS,
            None,
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()
    }
}

unsafe extern "system" fn taskband_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == taskband::SET_OVERLAY_ICON {
        // Copied now: the app may destroy its icon once this returns.
        let image = (l.0 != 0)
            .then(|| icons::icon_image(HICON(l.0 as *mut _)))
            .flatten();
        set(w.0 as isize, image);
        return LRESULT(0);
    }
    if taskband::CALLS.contains(&msg) {
        // Progress, thumbnail buttons, tabs: nothing shows them while the
        // taskbar is hidden. Answered with success.
        return LRESULT(0);
    }
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        },
        UI::{
            Shell::{ITaskbarList3, TaskbarList},
            WindowsAndMessaging::{
                DispatchMessageW, IDI_INFORMATION, LoadIconW, MSG, PM_REMOVE, PeekMessageW,
                SW_SHOWNOACTIVATE, ShowWindow, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
            },
        },
    };

    #[test]
    #[ignore = "Points the live taskbar's calls at the dock for a moment and back"]
    fn an_apps_overlay_icon_reaches_the_dock_and_goes_back_to_explorer() -> anyhow::Result<()> {
        // SAFETY: Test-owned COM apartment, windows and taskbar calls.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let instance = GetModuleHandleW(None)?;
            // An app's window, with a taskbar button, far off screen.
            let app = CreateWindowExW(
                WS_EX_APPWINDOW,
                windows::core::w!("STATIC"),
                windows::core::w!("Nosih overlay test"),
                WS_OVERLAPPEDWINDOW,
                -4000,
                -4000,
                200,
                100,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = ShowWindow(app, SW_SHOWNOACTIVATE);
            let notify = create_window().ok_or_else(|| anyhow::anyhow!("no window"))?;
            start(notify);
            // As Slack does: a taskbar list made after the dock started.
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            list.HrInit()?;
            let icon = LoadIconW(None, IDI_INFORMATION)?;
            list.SetOverlayIcon(app, icon, windows::core::w!("3 unread messages"))?;
            let pump = || {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
            };
            pump();
            let seen = image_for(std::iter::once(app.0 as isize));
            list.SetOverlayIcon(app, HICON::default(), None)?;
            pump();
            let cleared = image_for(std::iter::once(app.0 as isize)).is_none();
            stop();
            let _ = DestroyWindow(notify);
            let _ = DestroyWindow(app);
            let image = seen.ok_or_else(|| anyhow::anyhow!("no overlay reached the dock"))?;
            assert!(image.width > 0 && image.height > 0);
            assert!(cleared, "a removed overlay stays");
        }
        Ok(())
    }
}
