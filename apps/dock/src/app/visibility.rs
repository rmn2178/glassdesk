//! Monitor edges, intelligent hiding, fullscreen and reveal behavior.

use std::time::Instant;

use anyhow::Result;
use nosih_config::DockPosition;
use nosih_core::{frame::DockEdge, layout::Rect, spring::ElasticSpring};
use nosih_shell::{
    appbar::ABN_FULLSCREENAPP,
    system::{Monitor, monitor_at, monitor_rects, primary_monitor},
    taskbar, tracker,
};
use windows::Win32::{
    Foundation::{POINT, RECT},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
        WindowsAndMessaging::{GetCursorPos, KillTimer, SetTimer},
    },
};

use crate::{
    edge_watch::{self, EdgeWatch, Reveal, Target as EdgeTarget},
    menubar::MenuBar,
};

use super::{
    App, EDGE_TIMER, FULLSCREEN_RECHECK_MS, FULLSCREEN_TIMER, HIDE_DELAY_MS, HIDE_TIMER,
    HOVER_DAMPING, HOVER_OMEGA, millis,
};

pub(super) fn rects_overlap(a: RECT, b: RECT) -> bool {
    a.left < a.right
        && a.top < a.bottom
        && b.left < b.right
        && b.top < b.bottom
        && a.left < b.right
        && b.left < a.right
        && a.top < b.bottom
        && b.top < a.bottom
}

impl App {
    pub(super) fn hide_if_unused(&mut self) -> Result<()> {
        // One decision for timer, settings and overlap changes. Automatic
        // hiding wins; intelligent hiding matters only in its absence.
        let hidden = self.config.auto_hide || (self.config.smart_hide && self.smart_hidden);
        if hidden
            && (self.mascot.is_some()
                || self.menu_open
                || self.press.is_some()
                || self.drag_over
                || self.hovering
                || self.drag.is_some()
                || self.switching
                || self.preview.is_some()
                || self.group_view.is_some())
        {
            return Ok(());
        }
        if !hidden {
            // A queued hide must not survive returning to the desktop or
            // disabling a policy. Other interactions keep their own timers.
            unsafe {
                let _ = KillTimer(Some(self.input), HIDE_TIMER);
            }
        }
        let target = if hidden { 1.0 } else { 0.0 };
        if self.slide.target == target {
            return Ok(());
        }
        self.slide.target = target;
        self.last_frame = Instant::now();
        self.place_windows()
    }

    /// The screen edge chosen in Settings.
    pub(super) fn position(&self) -> DockEdge {
        match self.config.dock_position {
            DockPosition::Bottom => DockEdge::Bottom,
            DockPosition::Left => DockEdge::Left,
            DockPosition::Right => DockEdge::Right,
        }
    }

    /// How much of `screen`'s top the menu bar keeps, for a dock on a side
    /// edge to start below it. Zero on the bottom edge (not needed there),
    /// when the bar is off or hides itself, and on other displays (the bar
    /// is on the primary one).
    pub(super) fn menu_bar_inset(&self, screen: RECT, edge: DockEdge) -> i32 {
        if !edge.is_vertical() {
            return 0;
        }
        // From the settings, not the live bar: at startup the dock is placed
        // before the bar exists, and it must not move once the bar appears.
        if !self.config.menu_bar || self.config.menu_bar_auto_hide {
            return 0;
        }
        primary_monitor()
            .filter(|primary| primary.screen == screen)
            .map_or(0, |primary| MenuBar::strip_height(primary.scale))
    }

    pub(super) fn dock_monitor(&self) -> Option<Monitor> {
        self.dock_display
            .filter(|_| self.config.hides_automatically())
            .and_then(monitor_at)
            .or_else(primary_monitor)
    }

    pub(super) fn apply_edge_settings(&mut self) {
        if !self.config.hides_automatically() {
            self.dock_display = None;
        }
        let mut screens = if self.config.hides_automatically() {
            monitor_rects()
        } else {
            Vec::new()
        };
        let watch_dock = screens.len() > 1;
        if !watch_dock {
            // One display already has the dock's own reveal strip.
            screens.clear();
        }
        edge_watch::set_dock_monitors(screens, self.position());
        // Both hiding modes of the menu bar reveal it at the top edge; the
        // intelligent one while a window covers it (issue #61).
        let enabled = watch_dock
            || (self.menubar.is_some()
                && (self.config.menu_bar_auto_hide || self.config.menu_bar_smart_hide));
        if enabled && self.edge_watch.is_none() {
            self.edge_watch = EdgeWatch::start(self.visual)
                .inspect_err(|error| {
                    nosih_shell::log::line(&format!("bordes de pantalla: {error:#}"))
                })
                .ok();
        } else if !enabled && self.edge_watch.take().is_some() {
            // SAFETY: This timer belongs to our UI thread.
            unsafe {
                let _ = KillTimer(Some(self.visual), EDGE_TIMER);
            }
        }
    }

    pub(super) fn edge_changed(&mut self) -> Result<()> {
        // Explorer can recreate a secondary taskbar while the dock is running.
        // Check on monitor entry so it is transparent before the edge dwell.
        if self.config.hides_automatically()
            && self.taskbar_worker.is_none()
            && taskbar::needs_visibility_repair()
        {
            self.request_taskbar(false)?;
        }
        let (target, wait) = edge_watch::pending();
        let on_dock_edge = matches!(target, Some(EdgeTarget::Dock(screen))
            if self.stage.as_ref().is_some_and(|stage| stage.key.screen == screen));
        self.arm_edge_timer(wait);
        if !on_dock_edge
            && self.config.hides_automatically()
            && !self.hovering
            && self.slide.target == 0.0
        {
            // SAFETY: Existing hide timer on the dock input window.
            unsafe {
                SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None);
            }
        }
        Ok(())
    }

    pub(super) fn arm_edge_timer(&self, wait: Option<std::time::Duration>) {
        // SAFETY: One-shot timer on the window receiving edge notifications.
        unsafe {
            let _ = KillTimer(Some(self.visual), EDGE_TIMER);
            if let Some(wait) = wait {
                SetTimer(Some(self.visual), EDGE_TIMER, millis(wait).max(1), None);
            }
        }
    }

    pub(super) fn edge_rested(&mut self) -> Result<()> {
        self.arm_edge_timer(None);
        if self.edge_watch.is_none() {
            return Ok(());
        }
        let mut point = POINT::default();
        // SAFETY: Valid output for this call.
        unsafe { GetCursorPos(&mut point)? };
        match edge_watch::poll(point) {
            Reveal::Wait(wait) => self.arm_edge_timer(Some(wait)),
            Reveal::Ready(EdgeTarget::MenuBar) => {
                if let Some(bar) = &mut self.menubar {
                    bar.edge_rested(&self.gfx, point)?;
                }
            }
            Reveal::Ready(EdgeTarget::Dock(screen)) => self.reveal_dock(point, screen)?,
            Reveal::Idle => self.edge_changed()?,
        }
        Ok(())
    }

    pub(super) fn reveal_dock(&mut self, point: POINT, screen: RECT) -> Result<()> {
        // SAFETY: Read-only mouse button state.
        let mouse_down = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 });
        if !self.config.hides_automatically()
            || self.hovering
            || self.menu_open
            || self.press.is_some()
            || self.drag_over
            || self.drag.is_some()
            || self.switching
            || !self.flights.is_empty()
            || mouse_down
            || monitor_at(point).is_none_or(|monitor| monitor.screen != screen)
            || tracker::foreground_is_fullscreen(screen)
        {
            return Ok(());
        }
        let switched = self
            .stage
            .as_ref()
            .is_none_or(|stage| stage.key.screen != screen);
        if switched {
            self.close_preview();
            self.stop_mascot();
            self.dock_display = Some(POINT {
                x: screen.left + (screen.right - screen.left) / 2,
                y: screen.top + (screen.bottom - screen.top) / 2,
            });
            self.amount = ElasticSpring::at(0.0, HOVER_OMEGA, HOVER_DAMPING);
            self.slide = Spring::at(1.0, self.config.dock_auto_hide_speed.omega());
            self.fullscreen = false;
            self.refresh()?;
        }
        if self
            .stage
            .as_ref()
            .is_some_and(|stage| stage.spans_reveal(point))
        {
            return self.on_mouse_move();
        }
        self.slide.target = 0.0;
        self.last_frame = Instant::now();
        // SAFETY: Leaving the edge or entering the dock re-arms the hide
        // delay. Cancel any delay left over from the previous monitor.
        unsafe {
            let _ = KillTimer(Some(self.input), HIDE_TIMER);
        }
        self.place_windows()?;
        self.render()
    }

    pub(super) fn update_smart_hiding(&mut self) -> Result<()> {
        // Paused under automatic hiding: no competing overlap controller and
        // no native geometry query for a policy that cannot affect visibility.
        let covered = !self.config.auto_hide
            && self.config.smart_hide
            && self.stage.as_ref().is_some_and(|stage| {
                let front = tracker::foreground_window();
                if !self.windows.iter().any(|window| window.hwnd == front) {
                    return false;
                }
                // What shows of it: its invisible resize borders reaching
                // into the dock must not hide it.
                tracker::visible_rect(front).is_some_and(|rect| {
                    rects_overlap(
                        stage.screen_rect(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: stage.layout.width,
                            height: stage.layout.height,
                        }),
                        rect,
                    )
                })
            });
        if covered != self.smart_hidden {
            self.smart_hidden = covered;
            self.hide_if_unused()?;
            self.render()?;
        }
        Ok(())
    }

    pub(super) fn update_fullscreen(&mut self) -> Result<()> {
        // The window in front is where the user works: note where it is now.
        let front = tracker::foreground_window();
        if self.windows.iter().any(|window| window.hwnd == front) {
            self.remember_rect(front);
        }
        let Some(monitor) = primary_monitor() else {
            return Ok(());
        };
        let bar_fullscreen = tracker::foreground_is_fullscreen(monitor.screen);
        let fullscreen = self.dock_monitor().map_or(bar_fullscreen, |dock| {
            if dock.screen == monitor.screen {
                bar_fullscreen
            } else {
                tracker::foreground_is_fullscreen(dock.screen)
            }
        });
        if let Some(bar) = &mut self.menubar {
            // An auxiliary bar failure must not strand a hidden dock.
            if let Err(error) = bar.set_visible(!bar_fullscreen) {
                crate::recovery::graphics_error(&error);
                nosih_shell::log::line(&format!("visibilidad de la barra superior: {error:#}"));
            }
            // Runs when a window comes to the front and once it stops moving:
            // its title bar must not open, or be dropped, under the bar.
            if let Some(strip) = bar.reserved_strip() {
                tracker::keep_below(front, strip);
            }
            // Intelligent hiding (issue #61): the same test as the dock's,
            // against the bar's strip.
            let covered = self.config.menu_bar_smart_hide
                && self.windows.iter().any(|window| window.hwnd == front)
                && tracker::visible_rect(front)
                    .is_some_and(|rect| rects_overlap(bar.bounds(), rect));
            if let Err(error) = bar.set_covered(&self.gfx, covered) {
                nosih_shell::log::line(&format!("ocultación inteligente de la barra: {error:#}"));
            }
        }
        // Windows sometimes omits the shell event when a full-screen app
        // closes or switches desktops. Poll only while our UI is hidden.
        // SAFETY: `self.input` is a live window on this UI thread.
        unsafe {
            if fullscreen || (bar_fullscreen && self.menubar.is_some()) {
                SetTimer(
                    Some(self.input),
                    FULLSCREEN_TIMER,
                    FULLSCREEN_RECHECK_MS,
                    None,
                );
            }
        }
        if fullscreen != self.fullscreen {
            nosih_shell::log::line(&format!(
                "{} por pantalla completa (al frente: {})",
                if fullscreen { "se oculta" } else { "vuelve" },
                tracker::foreground_class()
            ));
            let previous = self.fullscreen;
            self.fullscreen = fullscreen;
            if fullscreen {
                self.stop_mascot();
                self.close_preview();
                self.close_group_view();
                self.hovering = false;
                self.amount = ElasticSpring::at(0.0, HOVER_OMEGA, HOVER_DAMPING);
                self.press = None;
                self.drag = None;
                self.ghost = None;
                self.stop_drag_input();
            }
            if let Err(error) = self.place_windows() {
                self.fullscreen = previous;
                return Err(error);
            }
        }
        if !fullscreen && (!bar_fullscreen || self.menubar.is_none()) {
            // SAFETY: This UI thread owns the timer.
            unsafe {
                let _ = KillTimer(Some(self.input), FULLSCREEN_TIMER);
            }
        }
        self.update_smart_hiding()?;
        // Updating an app name can fail transiently; it must never prevent
        // the dock or bar from returning after full-screen mode.
        if let Err(error) = self.update_menu_bar_app() {
            eprintln!("nosih: nombre de la barra superior: {error:#}");
        }
        Ok(())
    }

    pub(super) fn on_appbar_notice(&mut self, notice: usize) -> Result<()> {
        match notice {
            // Windows notifications may be missing or unreliable, so use them
            // only as a signal to recheck the state.
            ABN_FULLSCREENAPP => self.update_fullscreen(),
            // The dock's own reservation can trigger this. The strip changes
            // only with the screen or dock size, already handled by `refresh`.
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
