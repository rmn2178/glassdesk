//! Dock pointer tracking, presses and application activation choices.

use std::time::Instant;

use anyhow::Result;
use nosih_shell::{pins::app_exists, tracker};
use windows::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyState, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
            VK_CONTROL, VK_LBUTTON, VK_RBUTTON, VK_SHIFT,
        },
        WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, KillTimer, SM_SWAPBUTTON, SetTimer},
    },
};

use super::{
    App, DRAG_POLL_MS, DRAG_TIMER, Entry, HIDE_DELAY_MS, HIDE_TIMER, LEAVE_HALO, Outside, Press,
    Role,
    preferences::{explorer_id, save_settings},
    window_of,
};

pub(crate) fn primary_button_down() -> bool {
    // GetAsyncKeyState reports physical buttons; WM_LBUTTON* is logical.
    // Respect Windows' left-handed mouse configuration.
    // SAFETY: Read-only system metrics and button state.
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(key.0)) < 0
    }
}

pub(super) fn group_is_foreground(windows: &[isize], foreground: isize) -> bool {
    if !windows.contains(&foreground) {
        return false;
    }
    // The last app can still be reported as foreground after minimizing.
    // Decide from its current native state, not the tracker snapshot or the
    // foreground handle alone, or the next click only minimizes it again.
    let hwnd = HWND(foreground as *mut _);
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool()
            && !windows::Win32::UI::WindowsAndMessaging::IsIconic(hwnd).as_bool()
    }
}

impl App {
    pub(super) fn on_mouse_move(&mut self) -> Result<()> {
        // A player or browser can enter full screen without changing focus.
        // Do not let a stale reveal-strip message raise the dock above it.
        if !self.hovering {
            self.update_fullscreen()?;
        }
        if self.fullscreen {
            return Ok(());
        }
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        let mut point = POINT::default();
        // SAFETY: `point` remains alive for the duration of the call.
        unsafe { GetCursorPos(&mut point)? };

        let (along, _) = stage.to_dock(point);
        if self.update_drag(point) {
            self.cursor = along;
            return self.drag_to(point);
        }
        let Some(stage) = &self.stage else {
            return Ok(());
        };

        // Keep the active-area boundary aligned with the icons as they grow.
        let rise = stage.icon * (stage.magnifier.max_scale - 1.0) * self.amount.value.max(0.0);
        let (along, across) = stage.to_dock(point);
        if self.hovering && across < -rise - LEAVE_HALO * stage.scale {
            return self.on_mouse_leave();
        }

        let was_animating = self.is_animating();
        self.cursor = along;
        if !self.hovering {
            self.hovering = true;
            self.cursor_spring = nosih_core::spring::ElasticSpring::at(
                along,
                super::CURSOR_OMEGA,
                super::CURSOR_DAMPING,
            );
            self.amount.target = 1.0;
            self.slide.target = 0.0;
            if !was_animating {
                self.last_frame = Instant::now();
            }
            // SAFETY: Timer on our own window; killing a timer that is not set is harmless.
            unsafe {
                let _ = KillTimer(Some(self.input), HIDE_TIMER);
            }
            self.place_windows()?;
        } else {
            self.cursor_spring.target = along;
            if !was_animating {
                self.last_frame = Instant::now();
            }
        }
        // Every move, not only on entering: a menu or a drag that captures
        // the mouse cancels the request, and without it the dock would
        // never hear the pointer leave and stay magnified.
        self.track_leave()?;
        // When already animating, let `App::tick()` render once per DwmFlush VSync
        // instead of re-rendering synchronously at raw 125-1000 Hz mouse rate.
        if !was_animating {
            self.render()?;
        }
        self.aim_preview();
        Ok(())
    }

    pub(super) fn track_leave(&self) -> Result<()> {
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.input,
            dwHoverTime: 0,
        };
        // SAFETY: `track` is a valid structure with its size initialized.
        unsafe { TrackMouseEvent(&mut track)? };
        Ok(())
    }

    /// A menu closed: it held the mouse, so the dock heard nothing of where
    /// the pointer went meanwhile. Settle the magnifier on where it is now.
    pub(super) fn after_menu(&mut self) -> Result<()> {
        self.menu_open = false;
        let mut point = POINT::default();
        // SAFETY: Read-only queries.
        let over = unsafe {
            GetCursorPos(&mut point).is_ok()
                && windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(point) == self.input
        };
        if over {
            self.on_mouse_move()
        } else {
            self.on_mouse_leave()
        }
    }

    pub(super) fn on_tracked_mouse_leave(&mut self) -> Result<()> {
        let mut point = POINT::default();
        // Moving/resizing the magnified input area can queue a leave before
        // the pointer re-enters. Do not discard a newer hover/dwell on delivery.
        if unsafe { GetCursorPos(&mut point) }.is_ok()
            && unsafe { windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(point) }
                == self.input
        {
            return self.on_mouse_move();
        }
        self.on_mouse_leave()
    }

    pub(super) fn on_mouse_leave(&mut self) -> Result<()> {
        if !self.hovering || self.drag.is_some() || self.drag_over || self.menu_open {
            return Ok(());
        }
        self.hovering = false;
        if self.preview.is_none() {
            self.close_preview();
        }
        self.amount.target = 0.0;
        self.last_frame = Instant::now();
        if self.config.hides_automatically() {
            // SAFETY: Timer on our own window.
            unsafe { SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None) };
        }
        self.place_windows()?;
        self.render()
    }

    /// Only running or pinned apps, and groups, can be dragged. Explorer's
    /// built-in pin is managed by Settings; the other fixed items stay put too.
    pub(super) fn is_draggable(&self, entry: &Entry) -> bool {
        matches!(entry.role, Role::App | Role::Group)
            && entry.id.as_ref().is_some_and(|id| {
                *id != explorer_id() && (self.roster.is_pinned(id) || self.roster.is_running(id))
            })
    }

    pub(super) fn on_press(&mut self) -> Result<()> {
        if let Some(spotlight) = &mut self.spotlight {
            let _ = spotlight.close();
        }
        self.close_preview();
        // Any press on the dock closes an unfolded group; on the group
        // itself, `on_click` then leaves it closed.
        self.group_was_open = self.group_view.as_ref().map(|view| view.group_id.clone());
        self.close_group_view();
        let Some(at) = self.pointer_at_click()? else {
            return Ok(());
        };
        let Some(entry) = self.hovered.and_then(|index| self.entries.get(index)) else {
            return Ok(());
        };
        let draggable = self.is_draggable(entry);
        self.press_index = self.hovered;
        self.press_scale.target = 0.84;
        self.last_frame = Instant::now();
        self.press = entry.id.clone().map(|id| Press { id, at, draggable });
        if self.press.is_some() {
            // Capture is best effort: this non-activating window also polls
            // until release, including when the pointer leaves the dock.
            // Fixed icons need the same release/cancel path: otherwise a
            // button-up outside the dock can leave a press holding it open.
            // SAFETY: Both calls target our window on its owning thread.
            unsafe {
                SetCapture(self.input);
                SetTimer(Some(self.input), DRAG_TIMER, DRAG_POLL_MS, None);
            }
        }
        Ok(())
    }

    /// Button messages can arrive before a coalesced WM_MOUSEMOVE. Resolve
    /// the actual pointer again so a quick click never uses an older icon.
    pub(super) fn pointer_at_click(&mut self) -> Result<Option<POINT>> {
        let mut point = POINT::default();
        // SAFETY: Read-only queries with a live point buffer.
        unsafe {
            GetCursorPos(&mut point)?;
            if windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(point) != self.input {
                return Ok(None);
            }
        }
        self.hover_at(point)?;
        self.track_leave()?;
        Ok(Some(point))
    }

    pub(super) fn activate_for_pointer(&self) -> bool {
        let front = tracker::foreground_window();
        // Ordinary app clicks stay non-activating so clicking the active app
        // still minimizes it. Shell flyouts are not tracked application
        // windows: accepting this real click deactivates them before launch.
        self.stage.is_some()
            && front != 0
            && front != self.input.0 as isize
            && !tracker::is_desktop(HWND(front as *mut _))
            && !self.windows.iter().any(|window| window.hwnd == front)
    }

    pub(super) fn leave_if_outside(&mut self, point: POINT) -> Result<()> {
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        let zone = stage.hover_zone;
        let inside = point.x >= zone.left
            && point.x < zone.right
            && point.y >= zone.top
            && point.y < zone.bottom;
        if inside {
            Ok(())
        } else {
            self.on_mouse_leave()
        }
    }

    /// On icon click, launches a closed app or brings its windows to the
    /// foreground. Clicking the app that is already in front minimizes it, as
    /// the Windows taskbar does; the next click brings it back.
    pub(super) fn on_click(&mut self, pressed_id: &str) -> Result<Option<Outside>> {
        let group_was_open = self.group_was_open.take();
        if self.pointer_at_click()?.is_none() {
            return Ok(None);
        }
        let Some(entry) = self.hovered.and_then(|index| self.entries.get(index)) else {
            return Ok(None);
        };
        let (Some(id), role) = (entry.id.clone(), entry.role) else {
            return Ok(None);
        };
        if id != pressed_id {
            return Ok(None);
        }

        Ok(match role {
            Role::App => self.click_app(id),
            // A click unfolds the group; another click on it folds it back.
            Role::Group => {
                if group_was_open.as_deref() != Some(id.as_str()) {
                    self.open_group_view(&id)?;
                }
                None
            }
            Role::Start => Some(Outside::OpenStart),
            Role::Window => window_of(&id).and_then(|hwnd| self.activating(vec![hwnd])),
            Role::Shortcuts => self.folder_menu(id)?,
            Role::Folder | Role::Trash => Some(Outside::Open(id)),
        })
    }

    /// What clicking the app `id` does, in the dock or in an unfolded group:
    /// open it, bring its windows to the front, or minimize them if it is the
    /// app in front.
    pub(super) fn click_app(&mut self, id: String) -> Option<Outside> {
        let windows = self.windows_of(&id);
        // As on the Windows taskbar: Shift+click opens another window of the
        // app, Ctrl+Shift+click opens it as administrator.
        // SAFETY: Read-only keyboard state queries.
        let (shift, ctrl) = unsafe {
            (
                GetKeyState(i32::from(VK_SHIFT.0)) < 0,
                GetKeyState(i32::from(VK_CONTROL.0)) < 0,
            )
        };
        if shift {
            self.start_bounce(id.clone(), None);
            return Some(if ctrl {
                Outside::OpenAsAdmin(id)
            } else {
                Outside::Open(id)
            });
        }
        if windows.is_empty() && !app_exists(&id) {
            // Uninstalled while the dock was running: it leaves the dock
            // instead of failing to open.
            if self.prune_missing_apps() {
                save_settings(&self.roster, &self.config);
                if let Err(error) = self.refresh() {
                    nosih_shell::log::line(&format!("app desinstalada: {error:#}"));
                }
            }
            None
        } else if windows.is_empty() {
            self.start_bounce(id.clone(), None);
            Some(Outside::Open(id))
        } else if group_is_foreground(&windows, tracker::foreground_window()) {
            Some(Outside::Minimize(windows))
        } else {
            self.activating(windows)
        }
    }
}

#[cfg(test)]
mod activation_tests {
    use super::group_is_foreground;
    use windows::{
        Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, IsIconic, IsWindowVisible, SW_HIDE,
                SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_TOOLWINDOW,
                WS_OVERLAPPEDWINDOW,
            },
        },
        core::w,
    };

    struct Window(HWND);
    impl Window {
        fn new() -> windows::core::Result<Self> {
            // An off-screen test-owned tool window, never the user's apps.
            unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("Nosih activation test"),
                    WS_OVERLAPPEDWINDOW,
                    -32000,
                    -32000,
                    160,
                    120,
                    None,
                    None,
                    None,
                    None,
                )
                .map(Self)
            }
        }
        fn raw(&self) -> isize {
            self.0.0 as isize
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    #[test]
    #[ignore = "Requires Windows window management; modifies test-owned windows only"]
    fn a_minimized_foreground_window_must_restore_on_the_first_dock_click()
    -> windows::core::Result<()> {
        let target = Window::new()?;
        unsafe {
            let _ = ShowWindow(target.0, SW_SHOWMINNOACTIVE);
        }
        assert!(unsafe { IsIconic(target.0) }.as_bool());
        // Minimized windows retain WS_VISIBLE. Checking visibility or a
        // potentially stale tracker snapshot alone cannot settle this case.
        assert!(unsafe { IsWindowVisible(target.0) }.as_bool());
        assert!(
            !group_is_foreground(&[target.raw()], target.raw()),
            "a minimized foreground handle must not select Minimize again"
        );
        let peer = Window::new()?;
        unsafe {
            let _ = ShowWindow(peer.0, SW_SHOWNOACTIVATE);
        }
        assert!(!group_is_foreground(
            &[target.raw(), peer.raw()],
            target.raw()
        ));
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows window management; modifies test-owned windows only"]
    fn only_a_visible_restored_foreground_member_selects_minimize() -> windows::core::Result<()> {
        let target = Window::new()?;
        let other = Window::new()?;
        unsafe {
            let _ = ShowWindow(target.0, SW_SHOWNOACTIVATE);
        }
        assert!(group_is_foreground(&[target.raw()], target.raw()));
        assert!(!group_is_foreground(&[target.raw()], other.raw()));
        assert!(!group_is_foreground(&[], target.raw()));
        unsafe {
            let _ = ShowWindow(target.0, SW_HIDE);
        }
        assert!(!group_is_foreground(&[target.raw()], target.raw()));
        Ok(())
    }
}
