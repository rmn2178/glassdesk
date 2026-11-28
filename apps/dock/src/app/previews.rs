//! Window preview dwell, presentation and stable ordering tests.

use anyhow::Result;
use nosih_core::magnify::magnify;
use windows::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::{GetCursorPos, KillTimer, SetTimer},
};

use crate::{
    multi_window_view::{self, MultiWindowView},
    scene::Theme,
};

use super::{App, HIDE_DELAY_MS, HIDE_TIMER, PREVIEW_DWELL_MS, PREVIEW_TIMER, Role, window_of};

impl App {
    pub(super) fn preview_target(&self) -> Option<(&str, usize)> {
        if !self.hovering
            || self.menu_open
            || self.press.is_some()
            || self.drag.is_some()
            || self.drag_over
            || self.fullscreen
            || self.switching
            || self.group_view.is_some()
        {
            return None;
        }
        let index = self.hovered?;
        let entry = self.entries.get(index)?;
        let id = entry.id.as_deref()?;
        let has_windows = match entry.role {
            Role::App if entry.running => self
                .windows
                .iter()
                .filter(|window| window.app_id == id)
                .nth(0)
                .is_some(),
            Role::Window => {
                window_of(id).is_some_and(|hwnd| self.windows.iter().any(|w| w.hwnd == hwnd))
            }
            _ => false,
        };
        has_windows.then_some((id, index))
    }

    pub(super) fn aim_preview(&mut self) {
        let target = self.preview_target().map(|(id, _)| id);
        if self
            .preview
            .as_ref()
            .is_some_and(|picker| target.is_none_or(|id| id == picker.app_id))
            || target == self.preview_candidate.as_deref() && self.preview.is_none()
        {
            return;
        }
        let candidate = target.map(str::to_owned);
        self.close_preview();
        if candidate.is_some() {
            // SAFETY: The timer belongs to our UI window.
            unsafe { SetTimer(Some(self.input), PREVIEW_TIMER, PREVIEW_DWELL_MS, None) };
            self.preview_candidate = candidate;
        }
    }

    pub(super) fn close_preview(&mut self) {
        if self.preview_candidate.take().is_none() && self.preview.is_none() {
            return;
        }
        // SAFETY: The timer belongs to our UI window.
        unsafe {
            let _ = KillTimer(Some(self.input), PREVIEW_TIMER);
        }
        if self.preview.take().is_some() && self.config.hides_automatically() && !self.hovering {
            unsafe { SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None) };
        }
    }

    pub(super) fn preview_tick(&mut self) -> Result<()> {
        let mut point = POINT::default();
        // SAFETY: Read-only pointer and window queries on the UI thread.
        if unsafe { GetCursorPos(&mut point) }.is_err() {
            self.close_preview();
            return Ok(());
        }
        if self.preview.is_some() {
            return Ok(());
        }
        // SAFETY: End the one-shot dwell before creating the preview.
        unsafe {
            let _ = KillTimer(Some(self.input), PREVIEW_TIMER);
        }
        let Some(candidate) = self.preview_candidate.take() else {
            return Ok(());
        };
        let Some((id, index)) = self.preview_target() else {
            return Ok(());
        };
        if id != candidate
            || unsafe { windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(point) }
                != self.input
        {
            return Ok(());
        }
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        let state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value.max(0.0),
            &stage.magnifier,
        );
        let rect = &state.rects[index];
        let anchor = stage.screen_rect(*rect);
        let windows = if let Some(hwnd) = window_of(&candidate) {
            vec![hwnd]
        } else {
            self.preview_order.windows_of(&self.windows, &candidate)
        };
        if windows.is_empty() {
            return Ok(());
        }
        self.preview = Some(MultiWindowView::show(
            &self.gfx,
            candidate,
            windows,
            &multi_window_view::Anchor {
                icon: anchor,
                screen: stage.key.screen,
                edge: stage.frame.edge,
            },
            stage.scale,
            Theme::new(stage.key.look),
        )?);
        // The view owns its animation and dismissal timer while it is open.
        self.render()?;
        Ok(())
    }
}

#[cfg(test)]
mod preview_order_tests {
    use super::super::PreviewOrder;
    use nosih_shell::tracker::TrackedWindow;

    fn window(hwnd: isize, app: &str, minimized: bool) -> TrackedWindow {
        TrackedWindow {
            hwnd,
            app_id: app.into(),
            minimized,
        }
    }

    #[test]
    fn activation_and_minimizing_do_not_move_preview_cards() {
        let mut order = PreviewOrder::default();
        order.update(&[
            window(3, "browser", false),
            window(2, "browser", false),
            window(1, "browser", false),
        ]);
        let current = [
            window(1, "browser", false),
            window(3, "browser", true),
            window(2, "browser", false),
        ];
        order.update(&current);
        assert_eq!(order.windows_of(&current, "browser"), vec![1, 2, 3]);
        assert_eq!(
            current[0].hwnd, 1,
            "activation order must remain independent"
        );
    }

    #[test]
    fn newly_opened_windows_append_and_closed_windows_leave_no_stale_positions() {
        let mut order = PreviewOrder::default();
        order.update(&[window(2, "browser", false), window(1, "browser", false)]);
        let current = [window(3, "browser", false), window(1, "browser", false)];
        order.update(&current);
        assert_eq!(order.windows_of(&current, "browser"), vec![1, 3]);
        let current = [
            window(2, "browser", false),
            window(3, "browser", false),
            window(1, "browser", false),
        ];
        order.update(&current);
        assert_eq!(order.windows_of(&current, "browser"), vec![1, 3, 2]);
    }

    #[test]
    fn app_groups_keep_their_order_when_other_apps_change() {
        let mut order = PreviewOrder::default();
        order.update(&[
            window(3, "editor", false),
            window(2, "browser", false),
            window(1, "browser", false),
        ]);
        let current = [
            window(4, "editor", false),
            window(2, "browser", false),
            window(1, "browser", false),
        ];
        order.update(&current);
        assert_eq!(order.windows_of(&current, "browser"), vec![1, 2]);
        assert_eq!(order.windows_of(&current, "editor"), vec![4]);
        order.update(&[]);
        assert!(order.0.is_empty());
    }
}
