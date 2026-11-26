//! Window tracking, restore/minimize flights and Alt+Tab following.

use std::{sync::atomic::Ordering, time::Instant};

use anyhow::Result;
use nosih_core::bounce;
use nosih_shell::{
    launcher, status,
    system::{animations_enabled, set_window_transitions},
    tracker,
};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VIRTUAL_KEY, VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_LWIN, VK_MENU,
            VK_RIGHT, VK_RWIN, VK_SHIFT, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{KillTimer, SetTimer},
    },
};

use crate::flight::Flight;

use super::{
    App, HIDE_TIMER, LAST_FRONT, Outside, Role, SWITCHER_APPEAR, SWITCHER_ECHO, SWITCHER_POLL_MS,
    SWITCHER_TIMER, SwitcherFollow,
    feedback::active_attention_window,
    preferences::{explorer_id, save_settings},
    window_id,
};

/// Where the switcher's selection lands once the windows marked `false` in
/// `alive` close: it stays at the same place in the list (the next window
/// takes the closed one's place), or on the new last one. `None` if none is
/// left.
pub(super) fn selection_after_closing(index: usize, alive: &[bool]) -> Option<usize> {
    let left = alive.iter().filter(|&&on| on).count();
    if left == 0 {
        return None;
    }
    let closed_before = alive[..index.min(alive.len())]
        .iter()
        .filter(|&&on| !on)
        .count();
    Some((index - closed_before).min(left - 1))
}

impl App {
    /// Screen rectangle, at rest and with the window's aspect ratio, where a
    /// minimized window lives in the dock: its app's icon, or its own
    /// thumbnail slot with "show minimized windows". The genie flies into it
    /// and back out of it.
    pub(super) fn minimized_slot(&self, hwnd: isize, aspect: f32) -> Option<RECT> {
        let stage = self.stage.as_ref()?;
        let id = if !self.config.show_minimized {
            self.windows
                .iter()
                .find(|window| window.hwnd == hwnd)?
                .app_id
                .clone()
        } else {
            window_id(hwnd)
        };
        // An app in a group lives in its group's icon.
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id.as_deref() == Some(id.as_str()))
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|entry| entry.members.contains(&id))
            })?;
        let rect = stage.frame.rect(stage.layout.slots.get(index)?.rect);
        let (width, height) = if aspect >= 1.0 {
            (rect.width, rect.width / aspect)
        } else {
            (rect.height * aspect, rect.height)
        };
        let left = rect.x + (rect.width - width) / 2.0;
        let top = rect.y + (rect.height - height) / 2.0;
        Some(RECT {
            left: left as i32,
            top: top as i32,
            right: (left + width) as i32,
            bottom: (top + height) as i32,
        })
    }

    /// A window starts minimizing: put it in the dock right away and fly it there.
    pub(super) fn on_minimize_start(&mut self, hwnd: isize) -> Result<()> {
        if self.fullscreen
            || self.minimized.contains(&hwnd)
            || !self.windows.iter().any(|window| window.hwnd == hwnd)
        {
            return Ok(());
        }
        // The window has already vanished (its system transition is off):
        // the stand-in goes up before anything else, or the desktop shows.
        self.show_stand_ins(&[hwnd]);
        self.minimized.push(hwnd);
        self.sync_thumbnails();
        self.refresh()?;
        self.aim_flights();
        Ok(())
    }

    /// Live thumbnails exist only for minimized windows shown in the dock:
    /// none while they hide in their app's icon, so DWM draws nothing extra.
    pub(super) fn sync_thumbnails(&mut self) {
        let shown: &[isize] = if self.config.show_minimized {
            &self.minimized
        } else {
            &[]
        };
        self.thumbnails.sync(shown);
    }

    /// Turns off the system's transitions for every tracked window. Windows
    /// starts its own minimize animation before the dock hears about the
    /// minimize, and it cannot be cancelled afterwards, so it has to be off in
    /// advance for the genie to be the only animation. The cost: those windows
    /// also lose the system's maximize and close animations while the dock runs.
    pub(super) fn calm_windows(&mut self) {
        if !self.config.genie || !animations_enabled() {
            return;
        }
        let windows = &self.windows;
        self.calmed
            .retain(|hwnd| windows.iter().any(|window| window.hwnd == *hwnd));
        for window in windows {
            if !self.calmed.contains(&window.hwnd) {
                set_window_transitions(window.hwnd, false);
                self.calmed.push(window.hwnd);
            }
        }
    }

    pub(super) fn restore_transitions(&mut self) {
        for hwnd in self.calmed.drain(..) {
            set_window_transitions(hwnd, true);
        }
    }

    /// Brings windows to the front. A minimized one flies out of the dock if
    /// it flew in; otherwise it gets its system transitions back first, so it
    /// returns with the system's restore animation (the next `sync` turns
    /// them off again).
    pub(crate) fn activating(&mut self, windows: Vec<isize>) -> Option<Outside> {
        if self.start_restore_flight(&windows) {
            // `tick` restores the window when the genie reaches full size.
            return None;
        }
        for hwnd in &windows {
            if self.minimized.contains(hwnd) {
                set_window_transitions(*hwnd, true);
                self.calmed.retain(|calmed| calmed != hwnd);
            }
        }
        Some(Outside::Activate(windows))
    }

    /// Flies out of the dock the window that activating `windows` would
    /// restore. `false` if the genie does not apply and the caller must
    /// restore it directly.
    pub(super) fn start_restore_flight(&mut self, windows: &[isize]) -> bool {
        if !self.config.genie || !animations_enabled() || !self.flights.is_empty() {
            return false;
        }
        // Activation restores a window only when all of them are minimized,
        // and then only the first.
        let Some(&hwnd) = windows.first() else {
            return false;
        };
        // The tracker can lag behind a rapid restore/minimize. Never play an
        // old snapshot over a window that is already visible again.
        if !windows.iter().all(|window| unsafe {
            windows::Win32::UI::WindowsAndMessaging::IsIconic(HWND(*window as *mut _)).as_bool()
        }) {
            return false;
        }
        let Some(position) = self.snapshots.iter().position(|(owner, _)| *owner == hwnd) else {
            return false;
        };
        let Some(screen) = self.stage.as_ref().map(|stage| stage.key.screen) else {
            return false;
        };
        let flight = self.snapshots.get(position).and_then(|(_, snapshot)| {
            let slot = self.minimized_slot(hwnd, snapshot.aspect())?;
            Flight::restore(
                &self.gfx,
                hwnd,
                snapshot,
                slot,
                screen,
                self.config.genie_speed.seconds(),
                self.position(),
            )
        });
        let Some(flight) = flight else {
            return false;
        };
        if !self.config.show_minimized {
            flight.tuck_under(self.visual);
        }
        self.snapshots.remove(position);
        self.flights.push(flight);
        self.last_frame = Instant::now();
        true
    }

    pub(super) fn start_flights(&mut self, windows: &[isize]) {
        self.show_stand_ins(windows);
        self.aim_flights();
    }

    /// Gives each new minimize flight its slot, now that the dock shows it.
    pub(super) fn aim_flights(&mut self) {
        let slots: Vec<(usize, Option<RECT>)> = self
            .flights
            .iter()
            .enumerate()
            .filter(|(_, flight)| flight.needs_aim())
            .map(|(index, flight)| (index, self.minimized_slot(flight.hwnd, flight.aspect())))
            .collect();
        for (index, slot) in slots {
            if let (Some(slot), Some(flight)) = (slot, self.flights.get_mut(index)) {
                flight.aim(&self.gfx, slot);
                if !self.config.show_minimized {
                    flight.tuck_under(self.visual);
                }
            }
        }
    }

    pub(super) fn show_stand_ins(&mut self, windows: &[isize]) {
        // Respect "Animation effects" being turned off in Windows settings.
        // One genie at a time: minimizing everything at once (Win+D) would
        // otherwise take a screen snapshot per window.
        if !self.config.genie || !animations_enabled() || !self.flights.is_empty() {
            return;
        }
        let Some(screen) = self.stage.as_ref().map(|stage| stage.key.screen) else {
            return;
        };
        for &hwnd in windows {
            let last_seen = self.last_rects.get(&hwnd).copied();
            let Some((from, frame)) = tracker::rect_before_minimize(hwnd, last_seen) else {
                continue;
            };
            if let Some(flight) = Flight::start(
                &self.gfx,
                hwnd,
                from,
                frame,
                screen,
                self.config.genie_speed.seconds(),
                self.position(),
            ) {
                self.flights.push(flight);
                self.last_frame = Instant::now();
                break;
            }
        }
    }

    /// Alt+Tab (or Task View) opened: an auto-hidden dock comes up to show
    /// the open apps until the switcher closes. It cannot follow the
    /// selection: Windows 11's switcher reports it to no one (no accessible
    /// focus, selection or name on its items; checked with MSAA and UIA).
    pub(super) fn task_switcher_opened(&mut self) -> Result<()> {
        // Only Alt+Tab (and Ctrl+Alt+Tab). Task View shares the switcher's
        // window, and on Windows 11 its virtual desktops sit along the
        // bottom edge, where the dock would cover them.
        // SAFETY: Read-only keyboard state query.
        let alt = unsafe { GetAsyncKeyState(i32::from(VK_MENU.0)) < 0 };
        // After it closes, Windows sometimes hands the focus back to the
        // switcher's hidden window for a moment: not a new Alt+Tab.
        let echo = self
            .switch_ended
            .is_some_and(|ended| ended.elapsed() < SWITCHER_ECHO)
            && tracker::task_switcher_state() != Some(true);
        if self.switching || !alt || echo {
            return Ok(());
        }
        self.switching = true;
        self.close_preview();
        self.close_group_view();
        self.flights.clear();
        self.switch_started = Instant::now();
        // SAFETY: Timers on our own window.
        unsafe {
            SetTimer(Some(self.input), SWITCHER_TIMER, SWITCHER_POLL_MS, None);
            let _ = KillTimer(Some(self.input), HIDE_TIMER);
        }
        if self.config.hides_automatically() && !self.fullscreen {
            self.slide.target = 0.0;
            self.last_frame = Instant::now();
            self.place_windows()?;
            self.render()?;
        }
        if let Err(error) = self.start_following() {
            eprintln!("nosih: Alt+Tab: {error:#}");
        }
        Ok(())
    }

    pub(super) fn check_task_switcher(&mut self) -> Result<()> {
        let open = match tracker::task_switcher_state() {
            Some(shown) => shown || self.switch_started.elapsed() < SWITCHER_APPEAR,
            None => false,
        };
        if open {
            return self.follow_selection();
        }
        self.switching = false;
        self.switch_ended = Some(Instant::now());
        // SAFETY: Our own timer.
        unsafe {
            let _ = KillTimer(Some(self.input), SWITCHER_TIMER);
        }
        self.stop_following()?;
        self.hide_if_unused()
    }

    /// Starts following the switcher's selection, when it can be known:
    /// opened with Alt+Tab, and listing exactly this desktop's windows.
    ///
    /// Windows does not say which item is selected, so it is worked out the
    /// way the switcher moves: it lists the windows from the most recently
    /// used, starts on the second (the last with Alt+Shift+Tab), and each Tab
    /// moves one ahead, Shift+Tab one back.
    pub(super) fn start_following(&mut self) -> Result<()> {
        // SAFETY: Read-only keyboard state queries.
        let (alt, win, shift) = unsafe {
            (
                GetAsyncKeyState(i32::from(VK_MENU.0)) < 0,
                GetAsyncKeyState(i32::from(VK_LWIN.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_RWIN.0)) < 0,
                GetAsyncKeyState(i32::from(VK_SHIFT.0)) < 0,
            )
        };
        if !alt || win {
            return Ok(());
        }
        // Topmost first, which is the order of last use.
        let order: Vec<isize> = self
            .tracker
            .list()
            .iter()
            .map(|window| window.hwnd)
            .collect();
        let edge_open = self
            .windows
            .iter()
            .any(|window| window.app_id.ends_with("\\msedge.exe"));
        if order.len() < 2 || status::alt_tab_lists_more_than_windows(edge_open) {
            return Ok(());
        }
        // Opened from the desktop or another unlisted window, the switcher
        // starts on the first window rather than the second.
        let from_listed = order.first() == Some(&LAST_FRONT.load(Ordering::Relaxed));
        let index = match (shift, from_listed) {
            (true, _) => order.len() - 1,
            (false, true) => 1,
            (false, false) => 0,
        };
        self.follow = Some(SwitcherFollow {
            order,
            index,
            // The Tab that opened the switcher is still down: not a new press.
            tab_down: true,
        });
        self.point_at_selection()
    }

    /// Moves the selection with each Tab press. Arrow keys can change it in
    /// ways that cannot be followed: the highlight then goes away rather
    /// than point at the wrong app.
    pub(super) fn follow_selection(&mut self) -> Result<()> {
        let Some(follow) = &mut self.follow else {
            return Ok(());
        };
        let down = |key: VIRTUAL_KEY| {
            // SAFETY: Read-only keyboard state query.
            unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 }
        };
        // Only these move the selection in ways that cannot be followed. A
        // click on an item switches to it and closes the switcher; a click
        // anywhere else, or moving the pointer, leaves the selection alone.
        let lost = [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END]
            .into_iter()
            .any(down);
        if lost {
            return self.stop_following();
        }
        // A window closed from the switcher (its "x", or Delete) leaves the
        // list, and the switcher keeps the selection on the same place.
        let before = follow.order.len();
        let index = follow.index;
        let alive: Vec<bool> = follow
            .order
            .iter()
            .map(|&hwnd| {
                let window = HWND(hwnd as *mut _);
                // SAFETY: Read-only queries; a closed window simply fails them.
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)).as_bool()
                        && windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(window)
                            .as_bool()
                }
            })
            .collect();
        let mut position = 0;
        follow.order.retain(|_| {
            position += 1;
            alive[position - 1]
        });
        let closed = follow.order.len() != before;
        let Some(selected) = selection_after_closing(index, &alive) else {
            return self.stop_following();
        };
        follow.index = selected;
        let tab = down(VK_TAB);
        let pressed = tab && !follow.tab_down;
        follow.tab_down = tab;
        if !pressed {
            return if closed {
                self.point_at_selection()
            } else {
                Ok(())
            };
        }
        let count = follow.order.len();
        follow.index = if down(VK_SHIFT) {
            (follow.index + count - 1) % count
        } else {
            (follow.index + 1) % count
        };
        self.point_at_selection()
    }

    /// Magnifies the dock icon of the selected window, as if the pointer
    /// rested on it, and shows its name.
    pub(super) fn point_at_selection(&mut self) -> Result<()> {
        let Some(follow) = &self.follow else {
            return Ok(());
        };
        let selected = follow.order[follow.index];
        let window_entry = window_id(selected);
        let app = self
            .windows
            .iter()
            .find(|window| window.hwnd == selected)
            .map(|window| window.app_id.clone());
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id.as_deref() == Some(window_entry.as_str()))
            .or_else(|| {
                let app = app?;
                self.entries
                    .iter()
                    .position(|entry| entry.role == Role::App && entry.id.as_ref() == Some(&app))
                    .or_else(|| {
                        self.entries
                            .iter()
                            .position(|entry| entry.members.contains(&app))
                    })
            });
        let (Some(index), Some(stage)) = (index, &self.stage) else {
            return self.stop_following();
        };
        let Some(slot) = stage.layout.slots.get(index) else {
            return self.stop_following();
        };
        if self.hovering {
            return Ok(());
        }
        self.cursor = slot.rect.x + slot.rect.width / 2.0;
        self.pointed = true;
        self.amount.target = 1.0;
        self.last_frame = Instant::now();
        self.render()
    }

    pub(super) fn stop_following(&mut self) -> Result<()> {
        self.follow = None;
        if !self.pointed {
            return Ok(());
        }
        self.pointed = false;
        if !self.hovering {
            self.amount.target = 0.0;
            self.last_frame = Instant::now();
        }
        self.render()
    }

    pub(super) fn windows_of(&self, id: &str) -> Vec<isize> {
        self.windows
            .iter()
            .filter(|window| window.app_id == id)
            .map(|window| window.hwnd)
            .collect()
    }

    /// Refreshes open windows and recomposes the dock if anything changed.
    pub(super) fn sync(&mut self) -> Result<()> {
        let attention_changed = self.read_windows();
        if self
            .preview
            .as_ref()
            .is_some_and(|view| !view.matches(&self.windows_of(&view.app_id)))
        {
            self.close_preview();
        }
        let windows = &self.windows;
        self.last_rects
            .retain(|hwnd, _| windows.iter().any(|window| window.hwnd == *hwnd));
        let tracked: Vec<isize> = self.windows.iter().map(|window| window.hwnd).collect();
        for hwnd in tracked {
            self.remember_rect(hwnd);
        }
        self.calm_windows();
        let recent_before = self.roster.recent().to_vec();
        let changed = self
            .roster
            .sync(self.windows.iter().map(|window| window.app_id.as_str()));
        if !self.config.pin_explorer {
            self.roster.forget_recent(&explorer_id());
        }
        if self.roster.recent() != recent_before.as_slice() {
            save_settings(&self.roster, &self.config);
        }

        // The launching app has opened a window, so finish its current bounce.
        let now = Instant::now();
        for (id, bounce) in &mut self.bounces {
            if bounce.until.is_none() && self.roster.is_running(id) {
                let elapsed = now.duration_since(bounce.start).as_secs_f32();
                bounce.until = Some(bounce::end_of_hop(elapsed));
            }
        }

        let minimized_before = self.minimized.clone();
        let now_minimized: Vec<isize> = self
            .windows
            .iter()
            .filter(|window| window.minimized)
            .map(|window| window.hwnd)
            .collect();
        // A window still flying in was added at minimize start and may not
        // report as minimized yet.
        let flights = &self.flights;
        self.minimized.retain(|hwnd| {
            now_minimized.contains(hwnd) || flights.iter().any(|flight| flight.hwnd == *hwnd)
        });
        for hwnd in now_minimized {
            if !self.minimized.contains(&hwnd) {
                self.minimized.push(hwnd);
            }
        }
        self.sync_thumbnails();
        let minimized = &self.minimized;
        self.snapshots.retain(|(hwnd, _)| minimized.contains(hwnd));

        if changed || self.minimized != minimized_before {
            self.refresh()?;
        }
        let newly_minimized: Vec<isize> = self
            .minimized
            .iter()
            .copied()
            .filter(|hwnd| !minimized_before.contains(hwnd))
            .collect();
        if self.started && !self.fullscreen {
            self.start_flights(&newly_minimized);
        }
        self.started = true;
        if attention_changed {
            self.sync_badge_visuals()?;
            self.render()?;
        }
        // A foreground-window change may mean an app entered or exited full-screen mode.
        self.update_fullscreen()
    }

    pub(super) fn on_foreground_changed(&mut self) -> Result<()> {
        self.close_preview();
        let foreground = tracker::foreground_window();
        self.flights.retain(|flight| flight.is_current(foreground));
        // The app roster can stay identical while its windows change z-order.
        // Refresh their order so the next dock activation selects the actual
        // most recently used window rather than a stale group leader.
        let attention_changed = self.read_windows();
        #[cfg(feature = "recover-browser-z-order")]
        tracker::recover_browser_z_order(&self.windows, foreground);
        self.update_fullscreen()?;
        if attention_changed {
            self.sync_badge_visuals()?;
            self.render()?;
        }
        Ok(())
    }

    /// The hidden taskbar got the focus (see `taskbar::is_hidden`): it goes
    /// to the window in front instead, or to the desktop when none is left,
    /// so that no tooltip of the invisible taskbar shows (issue #48).
    pub(super) fn leave_hidden_taskbar(&mut self) -> Result<()> {
        self.on_foreground_changed()?;
        let windows: Vec<isize> = self.windows.iter().map(|window| window.hwnd).collect();
        launcher::focus_front_or_desktop(&windows);
        Ok(())
    }

    pub(super) fn read_windows(&mut self) -> bool {
        self.windows = self
            .tracker
            .list_with_workspaces(self.config.show_other_workspaces);
        self.preview_order.update(&self.windows);
        let pruned = self
            .attention
            .retain(|hwnd| self.windows.iter().any(|window| window.hwnd == hwnd));
        self.attention.acknowledge(active_attention_window(
            &self.windows,
            tracker::foreground_window(),
        )) || pruned
    }

    /// Notes where `hwnd` is on screen. A maximized window forgets it: its
    /// rectangle then comes from the work area, and a stale one from before
    /// could be taken after it is un-maximized.
    pub(super) fn remember_rect(&mut self, hwnd: isize) {
        match tracker::screen_rect(hwnd) {
            Some((rect, false)) => {
                self.last_rects.insert(hwnd, rect);
            }
            Some((_, true)) => {
                self.last_rects.remove(&hwnd);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod switcher_tests {
    use super::selection_after_closing;

    #[test]
    fn selection_after_closing_should_keep_its_place_in_the_list() {
        // Nothing closed.
        assert_eq!(selection_after_closing(1, &[true, true, true]), Some(1));
        // A window before the selection closes: the selection moves up one.
        assert_eq!(selection_after_closing(2, &[true, false, true]), Some(1));
        // A window after it closes: nothing moves.
        assert_eq!(selection_after_closing(0, &[true, true, false]), Some(0));
        // The selected window closes: the next one takes its place.
        assert_eq!(selection_after_closing(1, &[true, false, true]), Some(1));
        // The selected window was the last: the new last one.
        assert_eq!(selection_after_closing(2, &[true, true, false]), Some(1));
        // Everything closed.
        assert_eq!(selection_after_closing(0, &[false, false]), None);
    }
}
