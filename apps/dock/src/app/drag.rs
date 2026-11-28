//! Icon reordering and OLE file drops, including spring-loaded activation.

use std::time::Instant;

use anyhow::Result;
use nosih_core::{bounce, layout::Rect, magnify::magnify};
use nosih_shell::{
    files, pins,
    tracker::{self, normalize_id},
};
use windows::Win32::{
    Foundation::{LPARAM, POINT, WPARAM},
    System::Ole::DROPEFFECT,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, GetCapture, ReleaseCapture, VK_ESCAPE},
        WindowsAndMessaging::{
            GetCursorPos, GetSystemMetrics, KillTimer, PostMessageW, SM_CXDRAG, SetTimer,
        },
    },
};

use super::{
    App, DRAG_TIMER, DROP_RUN, Dragging, DropAction, HIDE_DELAY_MS, HIDE_TIMER, MERGE_SIDE,
    Outside, REMOVE_DISTANCE, Role, SPRING_DELAY_MS, SPRING_TIMER,
    input::{group_is_foreground, primary_button_down},
    preferences::{explorer_id, save_settings},
    window_of,
};

pub(super) fn group_drop_contains(rect: &Rect, x: f32, y: f32) -> bool {
    let side = rect.width * MERGE_SIDE;
    x >= rect.x + side && x <= rect.x + rect.width - side && y >= rect.y && y < rect.y + rect.height
}

impl App {
    /// Starts a drag once the cursor moves past the system drag threshold.
    /// Returns `true` while a drag is in progress.
    pub(super) fn update_drag(&mut self, point: POINT) -> bool {
        if self.drag.is_none()
            && let Some(press) = self.press.as_ref().filter(|press| press.draggable)
            && primary_button_down()
        {
            // SAFETY: Read-only query.
            let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(1);
            if (point.x - press.at.x).abs() > threshold || (point.y - press.at.y).abs() > threshold
            {
                self.drag = Some(Dragging {
                    id: press.id.clone(),
                    point,
                    removing: false,
                    merge: None,
                    from_group: false,
                });
            }
        }
        self.drag.is_some()
    }

    /// Moves the dragged icon and reorders apps live.
    /// Held over the middle of another pinned app or a group, the dragged
    /// app would join it in a group instead (see `MERGE_SIDE`).
    pub(super) fn drag_to(&mut self, point: POINT) -> Result<()> {
        let (Some(stage), Some(drag)) = (&self.stage, &mut self.drag) else {
            return Ok(());
        };
        drag.point = point;
        if let Some(ghost) = &self.ghost {
            ghost.move_to(point);
        }
        let id = drag.id.clone();
        let pinned = self.roster.is_pinned(&id);
        let group = self.roster.is_group(&id);
        // How far the pointer is from the dock, away from the screen edge.
        let (_, across) = stage.to_dock(point);
        let above = across < -stage.icon * (stage.magnifier.max_scale - 1.0 + REMOVE_DISTANCE);
        // A group never leaves the dock this way (it can be ungrouped from
        // its menu), and an app taken out of a group stays in the dock,
        // where it was last placed, wherever it is released.
        drag.removing = pinned && above && !group && !drag.from_group;
        drag.merge = None;
        if drag.removing || (above && drag.from_group) {
            return self.render();
        }

        let state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value.max(0.0),
            &stage.magnifier,
        );
        let explorer = explorer_id();
        let rise = stage.icon * (stage.magnifier.max_scale - 1.0);
        let in_dock = across >= -rise;
        if !group && in_dock {
            drag.merge = self
                .entries
                .iter()
                .zip(&state.rects)
                .find(|(entry, rect)| {
                    matches!(entry.role, Role::App | Role::Group)
                        && entry.id.as_ref().is_some_and(|other| {
                            *other != id && *other != explorer && self.roster.is_pinned(other)
                        })
                        && group_drop_contains(rect, self.cursor, across)
                })
                .and_then(|(entry, _)| entry.id.clone());
            if drag.merge.is_some() {
                return self.render();
            }
        }

        let mut before = None;
        let mut pinned_end = f32::MIN;
        let mut running_before = None;
        let mut running_end = f32::MIN;
        for (entry, rect) in self.entries.iter().zip(&state.rects) {
            let Some(other) = entry.id.as_deref() else {
                continue;
            };
            if !matches!(entry.role, Role::App | Role::Group) {
                continue;
            }
            let ahead = rect.x + rect.width / 2.0 > self.cursor;
            if self.roster.is_pinned(other) {
                if other != id {
                    pinned_end = rect.x + rect.width;
                    if other != explorer && ahead && before.is_none() {
                        before = Some(other);
                    }
                }
            } else if entry.running {
                running_end = rect.x + rect.width;
                if other != id && ahead && running_before.is_none() {
                    running_before = Some(other);
                }
            }
        }
        if !pinned {
            if !in_dock {
                return self.render();
            }
            // Moving among open apps keeps the icon unpinned. Explorer still
            // marks the end of the pinned section when it is the only pin.
            if before.is_none() && self.cursor > pinned_end + stage.gap {
                if self.cursor <= running_end + stage.gap
                    && self.roster.move_running_before(&id, running_before)
                {
                    return self.refresh();
                }
                return self.render();
            }
        }

        let list = self.roster.pinned();
        let current = list
            .iter()
            .position(|pinned| *pinned == id)
            .map(|index| list.get(index + 1).map(String::as_str));
        if current == Some(before) {
            return self.render();
        }
        self.roster.pin_before(&id, before);
        self.refresh()
    }

    /// Hover driven by a drag from another app: OLE takes the mouse, so the
    /// dock gets no WM_MOUSEMOVE meanwhile.
    pub(super) fn hover_at(&mut self, point: POINT) -> Result<()> {
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        let along = stage.to_dock(point).0;
        self.cursor = along;
        self.cursor_spring.target = along;
        if !self.hovering {
            self.hovering = true;
            self.amount.target = 1.0;
            self.slide.target = 0.0;
            self.last_frame = Instant::now();
            self.place_windows()?;
        }
        self.render()
    }

    pub(super) fn drop_action(&self) -> DropAction {
        if let Some(view) = &self.group_view
            && let Ok(Some((id, _))) = view.item_at_cursor()
            && (tracker::is_program(&id)
                || tracker::is_shortcut(&id)
                || pins::launch_target(&id).is_some())
        {
            return DropAction::OpenWith(id);
        }
        let Some(stage) = &self.stage else {
            return DropAction::Nothing;
        };
        let state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value.max(0.0),
            &stage.magnifier,
        );
        let target = state
            .icon_at(&stage.layout, self.cursor, stage.gap)
            .and_then(|index| self.entries.get(index));
        let Some(entry) = target else {
            return self.pin_action(&state);
        };
        let Some(id) = entry.id.clone() else {
            return DropAction::Nothing;
        };
        match entry.role {
            Role::App
                if tracker::is_program(&id)
                    || tracker::is_shortcut(&id)
                    || pins::launch_target(&id).is_some() =>
            {
                DropAction::OpenWith(id)
            }
            Role::Trash => DropAction::Recycle,
            Role::Folder => DropAction::Transfer {
                copy: !self
                    .incoming
                    .iter()
                    .all(|file| files::same_volume(file, &id)),
                folder: id,
            },
            // The user arranges the shortcuts folder in Explorer.
            Role::App | Role::Group | Role::Start | Role::Window | Role::Shortcuts => {
                DropAction::Nothing
            }
        }
    }

    /// A single app or shortcut dropped between icons is pinned there.
    pub(super) fn pin_action(&self, state: &nosih_core::magnify::Magnified) -> DropAction {
        let Some(id) = self.incoming_pin.clone() else {
            return DropAction::Nothing;
        };
        let explorer = explorer_id();
        let before = self
            .entries
            .iter()
            .zip(&state.rects)
            .find(|(entry, rect)| {
                matches!(entry.role, Role::App | Role::Group)
                    && entry.id.as_ref().is_some_and(|other| {
                        *other != explorer && *other != id && self.roster.is_pinned(other)
                    })
                    && rect.x + rect.width / 2.0 > self.cursor
            })
            .and_then(|(entry, _)| entry.id.clone());
        DropAction::Pin { id, before }
    }

    pub(super) fn on_drag_enter(
        &mut self,
        incoming: Vec<String>,
        point: POINT,
    ) -> Result<DROPEFFECT> {
        self.close_preview();
        // What a single dropped app or shortcut would pin, worked out once:
        // drag-over runs on every mouse move, and reading a shortcut is a
        // COM call and a disk read.
        self.incoming_pin = match incoming.as_slice() {
            [file] if tracker::is_shortcut(file) => pins::shortcut_id(std::path::Path::new(file)),
            [file] => Some(normalize_id(file)).filter(|id| id.ends_with(".exe")),
            _ => None,
        };
        self.incoming = incoming;
        self.drag_over = true;
        self.on_drag_over(point)
    }

    pub(super) fn on_drag_over(&mut self, point: POINT) -> Result<DROPEFFECT> {
        self.hover_at(point)?;
        if self.spring_opened_group {
            let over_group = self.group_view.as_ref().is_some_and(|v| v.contains(point));
            let over_dock = self.stage.as_ref().is_some_and(|stage| {
                let dock_point = stage.to_dock(point);
                dock_point.0 >= 0.0
                    && dock_point.0 < stage.layout.width
                    && dock_point.1 >= -stage.icon
                    && dock_point.1 < stage.layout.height
            });
            if !over_group && !over_dock {
                self.spring_opened_group = false;
                self.close_group_view();
            }
        }
        self.aim_spring();
        Ok(self.drop_action().effect())
    }

    pub(super) fn on_drag_leave(&mut self) -> Result<()> {
        self.incoming.clear();
        self.end_drag_over();
        self.on_mouse_leave()
    }

    pub(super) fn end_drag_over(&mut self) {
        self.drag_over = false;
        if self.spring.take().is_some() {
            // SAFETY: Our own timer on our own window.
            unsafe {
                let _ = KillTimer(Some(self.input), SPRING_TIMER);
            }
        }
        if self.spring_opened_group {
            self.spring_opened_group = false;
            self.close_group_view();
        }
    }

    /// The entry under a drag that can spring open: an app with windows,
    /// a minimized window's thumbnail, or a group of pinned apps.
    pub(super) fn spring_target(&self) -> Option<String> {
        if let Some(view) = &self.group_view
            && let Ok(Some((id, _))) = view.item_at_cursor()
            && !self.windows_of(&id).is_empty()
        {
            return Some(id);
        }
        let entry = self.hovered.and_then(|index| self.entries.get(index))?;
        let id = entry.id.clone()?;
        match entry.role {
            Role::App if !self.windows_of(&id).is_empty() => Some(id),
            Role::Window => Some(id),
            Role::Group => Some(id),
            _ => None,
        }
    }

    /// Starts the spring delay when the drag moves onto another entry; the
    /// delay restarts on each new entry, so sweeping across the dock opens
    /// nothing.
    pub(super) fn aim_spring(&mut self) {
        let target = self.spring_target();
        if target == self.spring {
            return;
        }
        // SAFETY: Our own timer on our own window; setting it again restarts it.
        unsafe {
            if target.is_some() {
                SetTimer(Some(self.input), SPRING_TIMER, SPRING_DELAY_MS, None);
            } else {
                let _ = KillTimer(Some(self.input), SPRING_TIMER);
            }
        }
        self.spring = target;
    }

    /// The drag rested on an app, a minimized window, or a group:
    /// - For a group: unfolds its member apps.
    /// - For a window: restores it to the front.
    /// - For an app: brings its windows to the front under the pointer so the drop
    ///   lands in the app; or cycles to the next window if already foreground.
    pub(super) fn spring_open(&mut self) -> Result<Option<Outside>> {
        // SAFETY: Our own timer on our own window.
        unsafe {
            let _ = KillTimer(Some(self.input), SPRING_TIMER);
        }
        if !self.drag_over || self.spring_target() != self.spring {
            return Ok(None);
        }
        let Some(id) = self.spring.clone() else {
            return Ok(None);
        };
        if let Some(view) = &self.group_view
            && let Ok(Some((item_id, _))) = view.item_at_cursor()
            && item_id == id
        {
            let windows = self.windows_of(&id);
            if windows.is_empty() {
                return Ok(None);
            }
            self.start_bounce(id.clone(), Some(bounce::HOP_SECONDS));
            return Ok(self.activating(windows));
        }
        let Some(entry) = self.hovered.and_then(|index| self.entries.get(index)) else {
            return Ok(None);
        };
        match entry.role {
            Role::Group => {
                self.start_bounce(id.clone(), Some(bounce::HOP_SECONDS));
                self.spring_opened_group = true;
                self.open_group_view(&id)?;
                Ok(None)
            }
            Role::Window => {
                let Some(hwnd) = window_of(&id) else {
                    return Ok(None);
                };
                self.start_bounce(id.clone(), Some(bounce::HOP_SECONDS));
                Ok(self.activating(vec![hwnd]))
            }
            Role::App => {
                let windows = self.windows_of(&id);
                if windows.is_empty() {
                    return Ok(None);
                }
                let foreground = tracker::foreground_window();
                let target_windows = if group_is_foreground(&windows, foreground) {
                    if windows.len() <= 1 {
                        return Ok(None);
                    }
                    let pos = windows.iter().position(|&w| w == foreground).unwrap_or(0);
                    let next = windows[(pos + 1) % windows.len()];
                    unsafe {
                        SetTimer(Some(self.input), SPRING_TIMER, SPRING_DELAY_MS, None);
                    }
                    vec![next]
                } else {
                    windows
                };
                self.start_bounce(id.clone(), Some(bounce::HOP_SECONDS));
                Ok(self.activating(target_windows))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn on_drop(&mut self, point: POINT) -> Result<DROPEFFECT> {
        self.hover_at(point)?;
        let action = self.drop_action();
        let effect = action.effect();
        let files = std::mem::take(&mut self.incoming);
        self.end_drag_over();
        self.pending_drop = match action {
            DropAction::Nothing => None,
            DropAction::OpenWith(app) => {
                self.start_bounce(app.clone(), None);
                Some(Outside::OpenWith(app, files))
            }
            DropAction::Recycle => Some(Outside::Recycle(files)),
            DropAction::Transfer { folder, copy } => Some(Outside::Transfer {
                folder,
                files,
                copy,
            }),
            DropAction::Pin { id, before } => {
                // A dropped shortcut that is needed to open the app (it has
                // arguments, or opens a script) is kept as Nosih's own copy,
                // so the pin keeps working if the original is deleted.
                let id = match files.first() {
                    Some(file) if tracker::is_shortcut(file) => {
                        pins::keep_shortcut(std::path::Path::new(file))
                    }
                    _ => Some(id),
                };
                if let Some(id) = id {
                    self.roster.pin_before(&id, before.as_deref());
                    save_settings(&self.roster, &self.config);
                    self.refresh()?;
                }
                None
            }
        };
        if self.pending_drop.is_some() {
            // SAFETY: Posting a message to our own window.
            unsafe {
                let _ = PostMessageW(Some(self.input), DROP_RUN, WPARAM(0), LPARAM(0));
            }
        }
        // The mouse is still over the dock; the next WM_MOUSEMOVE starts a
        // tracked hover again.
        self.on_mouse_leave()?;
        Ok(effect)
    }

    pub(super) fn on_release(&mut self) -> Result<Option<Outside>> {
        let press = self.press.take();
        if self.press_index.is_some() {
            self.press_scale.target = 1.0;
            self.press_scale.impulse(3.2);
            self.last_frame = Instant::now();
        }
        // Sample the release position too: the final WM_MOUSEMOVE can be
        // coalesced or delivered to another window outside this dock.
        let positioned = if self.drag.is_some() {
            self.on_mouse_move()
        } else {
            Ok(())
        };
        let drag = self.drag.take();
        self.ghost = None;
        self.stop_drag_input();
        positioned?;
        let Some(drag) = drag else {
            // A late button-up after polling/cancellation must never launch
            // an app, nor should releasing over a different icon.
            if let Some(press) = press {
                return self.on_click(&press.id);
            }
            return Ok(None);
        };
        if let Some(target) = &drag.merge {
            self.roster.group_with(&drag.id, target);
        } else if drag.removing {
            self.roster.unpin(&drag.id);
        }
        save_settings(&self.roster, &self.config);
        self.refresh()?;
        self.leave_if_outside(drag.point)?;
        self.after_group_drag(&drag)?;
        // The order may already match the live drag. Repaint anyway so the
        // icon settles into its slot even if the pointer stops moving.
        self.render()?;
        Ok(None)
    }

    /// Capture can be lost mid-drag (Alt+Tab, a system dialog): keep the order
    /// reached so far.
    pub(super) fn cancel_drag(&mut self) -> Result<()> {
        self.press = None;
        self.group_was_open = None;
        self.ghost = None;
        self.stop_drag_input();
        let Some(drag) = self.drag.take() else {
            return Ok(());
        };
        save_settings(&self.roster, &self.config);
        self.leave_if_outside(drag.point)?;
        self.after_group_drag(&drag)?;
        self.render()
    }

    pub(super) fn stop_drag_input(&self) {
        // Never release capture now held by a menu or a different popup.
        // SAFETY: Our timer and the current thread's capture window.
        unsafe {
            let _ = KillTimer(Some(self.input), DRAG_TIMER);
            // The hide delay may have expired while a press held the dock
            // open. Resume it once capture/polling ends, including cancellation.
            if self.config.hides_automatically() && !self.hovering {
                SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None);
            }
            if GetCapture() == self.input {
                let _ = ReleaseCapture();
            }
        }
    }

    pub(super) fn poll_drag(&mut self) -> Result<Option<Outside>> {
        if self.press.is_none() && self.drag.is_none() {
            self.stop_drag_input();
            return Ok(None);
        }
        // SAFETY: Read-only physical button state, independent of capture.
        let (down, escape) = unsafe {
            (
                primary_button_down(),
                GetAsyncKeyState(i32::from(VK_ESCAPE.0)) < 0,
            )
        };
        if escape {
            self.cancel_drag()?;
        } else if !down {
            // Button-up may still be queued. Resolve the release once here;
            // on_release consumes the press, so a late message is harmless.
            return self.on_release();
        } else {
            let mut point = POINT::default();
            // SAFETY: Valid output for this query.
            unsafe { GetCursorPos(&mut point)? };
            let previous = self
                .drag
                .as_ref()
                .map(|drag| drag.point)
                .or_else(|| self.press.as_ref().map(|press| press.at));
            if previous != Some(point) {
                self.on_mouse_move()?;
            }
        }
        Ok(None)
    }

    /// A drag out of a group starts away from the dock, so no hover ends
    /// with it: the input area shrinks back now, and an auto-hidden dock
    /// goes down.
    pub(super) fn after_group_drag(&mut self, drag: &Dragging) -> Result<()> {
        if !drag.from_group || self.hovering {
            return Ok(());
        }
        if self.config.hides_automatically() {
            // SAFETY: Timer on our own window.
            unsafe { SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None) };
        }
        self.place_windows()
    }
}

#[cfg(test)]
mod group_drop_tests {
    use super::{Rect, group_drop_contains};

    #[test]
    fn grouping_requires_the_pointer_over_the_icon_not_another_monitor_below_it() {
        let icon = Rect {
            x: 80.0,
            y: -24.0,
            width: 72.0,
            height: 72.0,
        };
        assert!(group_drop_contains(&icon, 116.0, 12.0));
        for y in [-25.0, 48.0, 200.0] {
            assert!(
                !group_drop_contains(&icon, 116.0, y),
                "outside the icon at y={y}"
            );
        }
    }

    #[test]
    fn the_sides_of_an_icon_remain_available_for_reordering() {
        let icon = Rect {
            x: 80.0,
            y: -24.0,
            width: 72.0,
            height: 72.0,
        };
        for x in [79.0, 81.0, 151.0, 153.0] {
            assert!(!group_drop_contains(&icon, x, 12.0));
        }
    }
}

#[cfg(test)]
mod spring_loaded_tests {
    #[test]
    fn window_cycling_rotates_through_open_windows() -> Result<(), &'static str> {
        let windows = [101, 102, 103];
        let foreground = 101;
        let pos = windows
            .iter()
            .position(|&w| w == foreground)
            .ok_or("foreground window missing")?;
        let next = windows[(pos + 1) % windows.len()];
        assert_eq!(next, 102);

        let pos = windows
            .iter()
            .position(|&w| w == 103)
            .ok_or("last window missing")?;
        let next = windows[(pos + 1) % windows.len()];
        assert_eq!(next, 101);
        Ok(())
    }

    #[test]
    fn single_window_does_not_cycle() {
        let windows = [101];
        let foreground = 101;
        let should_cycle = windows.len() > 1 && windows.contains(&foreground);
        assert!(!should_cycle);
    }
}
