//! Attention badges, bounce, mascot power moments and recycle-bin work.

use std::{sync::atomic::Ordering, thread, time::Instant};

use anyhow::Result;
use nosih_core::{attention::Attention, bounce};
use nosih_shell::{
    power, recycle_bin, status,
    system::animations_enabled,
    tracker::{self, TrackedWindow},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    UI::WindowsAndMessaging::{GetCursorPos, KillTimer, PostMessageW, SetTimer},
};

use crate::{mascot, menubar::MenuBar, moments};

use super::{
    ATTENTION_HOPS, App, BIN_QUERY, BIN_STATE, BREAK_CHECK_MS, BREAK_TIMER, Bounce, CHARGING_AGAIN,
    Entry, HIDE_DELAY_MS, HIDE_TIMER, MAIN_WINDOW, MASCOT_POLL_MS, MASCOT_TIMER,
    RUNNING_LAUNCH_HOPS, Role, WELCOME_DELAY_MS, WELCOME_RETRY_MS, WELCOME_TIMER, WELCOME_TRIES,
    window_of,
};

pub(super) fn app_has_attention(
    attention: &Attention,
    windows: &[TrackedWindow],
    id: &str,
) -> bool {
    windows
        .iter()
        .any(|window| window.app_id == id && attention.contains(window.hwnd))
}

/// The overlay icon an entry's app put on its taskbar button, if any (see
/// `overlays`): for a group, its first app that has one.
pub(super) fn entry_overlay(
    windows: &[TrackedWindow],
    entry: &Entry,
) -> Option<std::rc::Rc<nosih_shell::icons::IconImage>> {
    let of_app = |id: &str| {
        windows
            .iter()
            .filter(|window| window.app_id == id)
            .map(|window| window.hwnd)
            .collect::<Vec<_>>()
    };
    match entry.role {
        Role::App => crate::overlays::image_for(of_app(entry.id.as_deref()?).into_iter()),
        Role::Group => entry
            .members
            .iter()
            .find_map(|id| crate::overlays::image_for(of_app(id).into_iter())),
        Role::Window => {
            crate::overlays::image_for(entry.id.as_deref().and_then(window_of).into_iter())
        }
        _ => None,
    }
}

pub(super) fn entry_has_attention(
    attention: &Attention,
    windows: &[TrackedWindow],
    entry: &Entry,
) -> bool {
    match entry.role {
        Role::App => entry
            .id
            .as_deref()
            .is_some_and(|id| app_has_attention(attention, windows, id)),
        Role::Group => entry
            .members
            .iter()
            .any(|id| app_has_attention(attention, windows, id)),
        Role::Window => entry
            .id
            .as_deref()
            .and_then(window_of)
            .is_some_and(|hwnd| attention.contains(hwnd)),
        _ => false,
    }
}

pub(super) fn active_attention_window(windows: &[TrackedWindow], foreground: isize) -> isize {
    // Windows may briefly report its last minimized window as foreground.
    if windows
        .iter()
        .any(|window| window.hwnd == foreground && !window.minimized)
    {
        foreground
    } else {
        0
    }
}

pub(super) fn power_act(
    before: Option<(Option<bool>, bool)>,
    now: (Option<bool>, bool),
    charging_allowed: bool,
) -> Option<mascot::Act> {
    let before = before?;
    // Unplugging can also enable energy saver. Keep the charger reaction
    // instead of replacing it with the less specific saver-only moment.
    if now.0 == Some(false) && before.0 == Some(true) {
        Some(mascot::Act::Unplugged)
    } else if now.1 && !before.1 {
        Some(mascot::Act::Sleepy)
    } else if now.0 == Some(true) && before.0 == Some(false) && charging_allowed {
        Some(mascot::Act::Charging)
    } else {
        None
    }
}

/// Checks the recycle bin on a background thread by scanning each drive
/// (measured at 50–75 ms), so the UI is not blocked. The result arrives via
/// `BIN_STATE`.
pub(super) fn query_bin_in_background() {
    if !BIN_QUERY.request() {
        return;
    }
    let window = MAIN_WINDOW.load(Ordering::Relaxed);
    let spawned = thread::Builder::new()
        .name("nosih-recycle-query".into())
        .spawn(move || {
            power::set_thread_efficiency(true);
            // SAFETY: COM is initialized and released on this thread; the message
            // is harmless if the window no longer exists.
            unsafe {
                let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                loop {
                    BIN_QUERY.begin_pass();
                    let full = recycle_bin::has_items();
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        BIN_STATE,
                        WPARAM(usize::from(full)),
                        LPARAM(0),
                    );
                    if !BIN_QUERY.needs_another_pass() {
                        break;
                    }
                }
                if com.is_ok() {
                    CoUninitialize();
                }
            }
        });
    if let Err(error) = spawned {
        BIN_QUERY.0.store(0, Ordering::Release);
        eprintln!("nosih: no se pudo consultar la Papelera: {error}");
    }
}

impl App {
    /// The mascot plays `act` (see `mascot`), unless it is out already, it is
    /// turned off, or something runs full screen.
    pub(super) fn perform(&mut self, act: mascot::Act, place: mascot::Place) {
        if !self.can_perform() {
            return;
        }
        let scale = self.stage.as_ref().map_or(1.0, |stage| stage.scale);
        let character = self.config.mascot_character;
        let Some(mascot) = mascot::Mascot::play(&self.gfx, character, act, place, scale) else {
            return;
        };
        self.mascot = Some(mascot);
        // SAFETY: A timer on our own window, killed when the act ends.
        unsafe {
            if SetTimer(Some(self.input), MASCOT_TIMER, MASCOT_POLL_MS, None) == 0 {
                self.stop_mascot();
                return;
            }
        }
        if matches!(place, mascot::Place::Along { .. }) && self.config.hides_automatically() {
            // Reveal only after the mascot and its cleanup timer exist.
            unsafe {
                let _ = KillTimer(Some(self.input), HIDE_TIMER);
            }
            self.slide.target = 0.0;
            self.last_frame = Instant::now();
            if let Err(error) = self.place_windows().and_then(|()| self.render()) {
                nosih_shell::log::line(&format!("mostrar el dock para la mascota: {error:#}"));
                self.stop_mascot();
            }
        }
        if self.mascot.is_some() && act == mascot::Act::Charging {
            self.charging_shown = Some(Instant::now());
        }
    }

    pub(super) fn can_perform(&self) -> bool {
        self.config.mascot && self.mascot.is_none() && !self.fullscreen && animations_enabled()
    }

    pub(super) fn stop_mascot(&mut self) {
        let active = self.mascot.take().is_some();
        // SAFETY: These timers belong to this UI window. Always clear a stale
        // mascot timer, and resume the usual dock hiding delay after an act.
        unsafe {
            let _ = KillTimer(Some(self.input), MASCOT_TIMER);
            if active && self.config.hides_automatically() {
                SetTimer(Some(self.input), HIDE_TIMER, HIDE_DELAY_MS, None);
            }
        }
    }

    /// The dock's inner rim, for the crab to walk on. An auto-hidden
    /// dock comes up for the act and goes down after it, as the macOS Dock
    /// does when something asks for attention.
    pub(super) fn dock_edge(&self) -> Option<mascot::Place> {
        let stage = self.stage.as_ref()?;
        let margin = stage.icon / 2.0;
        self.rim(-margin, stage.layout.width + margin)
    }

    /// The rim over the icon `id`, for an act on it (the recycle bin).
    pub(super) fn rim_over(&self, id: &str) -> Option<mascot::Place> {
        let stage = self.stage.as_ref()?;
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id.as_deref() == Some(id))?;
        let icon = stage.layout.slots.get(index)?.rect;
        // Room for the mascot, centred on the icon.
        let room = stage.icon * 1.5;
        self.rim(icon.x - room, icon.x + icon.width + room)
    }

    /// The dock's inner rim from `from` to `to` along it, in dock pixels.
    fn rim(&self, from: f32, to: f32) -> Option<mascot::Place> {
        let frame = self.stage.as_ref()?.frame;
        // The rim is the dock's side away from the screen edge (dock
        // coordinate across = 0), and the walk runs along the dock.
        let start = frame.point(from, 0.0);
        let end = frame.point(to, 0.0);
        let (from, to, rim) = if frame.edge.is_vertical() {
            (start.1, end.1, start.0)
        } else {
            (start.0, end.0, start.1)
        };
        Some(mascot::Place::Along {
            left: from as i32,
            right: to as i32,
            top: rim as i32,
            edge: frame.edge,
        })
    }

    /// Under a point of the menu bar, or on the dock when the bar does not
    /// show: for the battery's and the network's moments.
    fn under_or_dock(&self, point: Option<POINT>) -> Option<mascot::Place> {
        point
            .map(|point| mascot::Place::Under {
                x: point.x,
                top: point.y,
            })
            .or_else(|| self.dock_edge())
    }

    pub(super) fn mascot_tick(&mut self) -> Result<()> {
        if !self.config.mascot
            || self.fullscreen
            || !animations_enabled()
            || self.mascot.as_ref().is_none_or(mascot::Mascot::finished)
        {
            self.stop_mascot();
            return Ok(());
        }
        let mut point = POINT::default();
        // SAFETY: A valid output for the pointer position.
        if unsafe { GetCursorPos(&mut point) }.is_ok()
            && let Some(mascot) = &mut self.mascot
        {
            mascot.poke(&self.gfx, point);
        }
        Ok(())
    }

    /// A moment found by the menu bar (an AI plan's figure): the crab comes
    /// out under that provider.
    pub(super) fn take_bar_cue(&mut self) {
        let cue = self.menubar.as_mut().and_then(MenuBar::take_cue);
        let Some((act, point)) = cue else {
            return;
        };
        // An AI plan's moment belongs under its figure in the island; the
        // network's comes to the dock when the bar does not show.
        let place = if matches!(act, mascot::Act::Offline | mascot::Act::Online) {
            self.under_or_dock(point)
        } else {
            point.map(|point| mascot::Place::Under {
                x: point.x,
                top: point.y,
            })
        };
        if let Some(place) = place {
            self.perform(act, place);
        }
    }

    /// The charger plugged in or out, or energy saver turned on: the crab walks
    /// along the dock. Only on the change itself, never at startup.
    pub(super) fn power_moment(&mut self) {
        let now = (
            nosih_shell::system::battery().map(|battery| battery.charging),
            nosih_shell::system::energy_saver_on(),
        );
        let before = self.power_seen.replace(now);
        let charging_allowed = self
            .charging_shown
            .is_none_or(|shown| shown.elapsed() >= CHARGING_AGAIN);
        let act = power_act(before, now, charging_allowed);
        let battery =
            nosih_shell::system::battery().map(|battery| (battery.percent, battery.charging));
        // Read on every change, so the next one compares with this.
        let level = self.moments.battery(battery);
        if act.is_none()
            && let Some(level) = level
            && self.can_perform()
        {
            // Under the battery icon, where its figure is.
            let point = self.menubar.as_ref().and_then(MenuBar::battery_anchor);
            if let Some(place) = self.under_or_dock(point) {
                self.perform(level, place);
            }
        }
        if let Some(act) = act {
            if matches!(act, mascot::Act::Charging | mascot::Act::Unplugged)
                && self.config.mascot
                && !self.fullscreen
                && animations_enabled()
            {
                // A disconnect must still show if it happens during the
                // charging reaction; keep only one scene and cleanup timer.
                self.stop_mascot();
            }
            if self.can_perform()
                && let Some(place) = self.dock_edge()
            {
                self.perform(act, place);
            }
        }
    }

    pub(super) fn set_bin_full(&mut self, full: bool) -> Result<()> {
        if full == self.bin_full {
            return Ok(());
        }
        let emptied = self.bin_full && !full;
        self.bin_full = full;
        self.assets.remove(recycle_bin::PARSING_NAME);
        self.refresh()?;
        // Emptied: a proud hop on the recycle bin itself.
        if emptied
            && self
                .moments
                .tidied
                .is_none_or(|tidied| tidied.elapsed() >= moments::TIDY_AGAIN)
            && self.can_perform()
            && let Some(place) = self.rim_over(recycle_bin::PARSING_NAME)
        {
            self.perform(mascot::Act::Tidy, place);
            if self.mascot.is_some() {
                self.moments.tidied = Some(Instant::now());
            }
        }
        Ok(())
    }

    /// The first moment of this run soon (see `welcome`): after start, a
    /// wake-up or an unlock.
    pub(super) fn arm_welcome(&mut self) {
        self.welcome_tries = WELCOME_TRIES;
        // SAFETY: A timer on our own window.
        unsafe {
            SetTimer(Some(self.input), WELCOME_TIMER, WELCOME_DELAY_MS, None);
        }
    }

    /// A new version's celebration in the middle of the dock, or the day's
    /// greeting across it; each once (see `moments::welcome`).
    pub(super) fn welcome(&mut self) -> Result<()> {
        // SAFETY: A timer on our own window.
        unsafe {
            let _ = KillTimer(Some(self.input), WELCOME_TIMER);
        }
        let mut memory = moments::Memory::load();
        let version = env!("CARGO_PKG_VERSION");
        let today = moments::today();
        let act = moments::welcome(&memory, &today, version);
        if !self.config.mascot || act.is_none() {
            // Nothing to show; a version seen without the mascot is not
            // celebrated later.
            if memory.version.as_deref() != Some(version) {
                memory.version = Some(version.to_owned());
                memory.save();
            }
            return Ok(());
        }
        let Some(act) = act else {
            return Ok(());
        };
        if !self.can_perform() {
            // Busy or full screen: later, a few times.
            if self.welcome_tries > 0 && animations_enabled() {
                self.welcome_tries -= 1;
                // SAFETY: A timer on our own window.
                unsafe {
                    SetTimer(Some(self.input), WELCOME_TIMER, WELCOME_RETRY_MS, None);
                }
            }
            return Ok(());
        }
        // The whole dock is the stage: the greeting crosses it, the update
        // celebrates in its middle.
        let Some(place) = self.dock_edge() else {
            return Ok(());
        };
        self.perform(act, place);
        if self.mascot.is_some() {
            memory.version = Some(version.to_owned());
            memory.greeted = Some(today);
            memory.save();
        }
        Ok(())
    }

    /// Takes the taskbar's overlay calls while Windows shows taskbar badges,
    /// so each app's unread count shows on its dock icon (see `overlays`);
    /// gives them back to Explorer when they are turned off.
    pub(super) fn sync_taskband(&mut self) {
        if self.attention_enabled {
            crate::overlays::start(HWND(MAIN_WINDOW.load(Ordering::Relaxed) as *mut _));
        } else {
            crate::overlays::stop();
        }
    }

    /// Starts or stops the break reminder's minute (see `break_tick`).
    pub(super) fn sync_break_timer(&mut self) {
        // SAFETY: A timer on our own window.
        unsafe {
            if self.config.mascot && self.config.mascot_break_reminder {
                SetTimer(Some(self.input), BREAK_TIMER, BREAK_CHECK_MS, None);
            } else {
                let _ = KillTimer(Some(self.input), BREAK_TIMER);
                self.moments.rested();
            }
        }
    }

    /// Once a minute while the break reminder is on: after a long stretch
    /// without a pause, the mascot yawns along the dock. Not during full
    /// screen (a call, a film), which counts as the reminder's turn.
    pub(super) fn break_tick(&mut self) -> Result<()> {
        use windows::Win32::{
            System::SystemInformation::GetTickCount,
            UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        };
        let mut last = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `last` has its size set and lives for the call.
        let idle = if unsafe { GetLastInputInfo(&mut last) }.as_bool() {
            // SAFETY: No arguments.
            let now = unsafe { GetTickCount() };
            std::time::Duration::from_millis(u64::from(now.wrapping_sub(last.dwTime)))
        } else {
            std::time::Duration::ZERO
        };
        if self.moments.break_due(idle, Instant::now())
            && self.can_perform()
            && let Some(place) = self.dock_edge()
        {
            self.perform(mascot::Act::Rest, place);
        }
        Ok(())
    }

    /// `until: None` is the bounce of an app being opened, which the user can
    /// turn off; a request for attention always bounces.
    ///
    /// An app that already runs without windows, in the notification area,
    /// may open none (Rainmeter shows its window only from its tray icon,
    /// issue #59): it bounces twice, not until the limit, as if it hung.
    pub(super) fn start_bounce(&mut self, id: String, until: Option<f32>) {
        if until.is_none() && !self.config.bounce_on_open {
            return;
        }
        if self.bounces.contains_key(&id) {
            return;
        }
        let until = until.or_else(|| {
            tracker::is_running(&id).then_some(RUNNING_LAUNCH_HOPS * bounce::HOP_SECONDS)
        });
        self.bounces.entry(id).or_insert(Bounce {
            start: Instant::now(),
            until,
        });
        self.last_frame = Instant::now();
    }

    pub(super) fn on_attention(&mut self, hwnd: isize) -> Result<()> {
        if !self.windows.iter().any(|window| window.hwnd == hwnd) && self.read_windows() {
            self.sync_badge_visuals()?;
            self.render()?;
        }
        // A minimize/flash can arrive before the debounced tracker refresh.
        // SAFETY: Read-only query; a stale or destroyed HWND returns false.
        let minimized =
            unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(HWND(hwnd as *mut _)) }
                .as_bool();
        let foreground = if minimized {
            0
        } else {
            active_attention_window(&self.windows, tracker::foreground_window())
        };
        if hwnd == foreground {
            return Ok(());
        }
        let app_id = self
            .windows
            .iter()
            .find(|window| window.hwnd == hwnd)
            .map(|window| window.app_id.clone());
        if let Some(app_id) = app_id {
            let changed = self.attention_enabled && self.attention.request(hwnd, foreground);
            if (changed || !self.attention_enabled) && animations_enabled() {
                self.start_bounce(app_id.clone(), Some(ATTENTION_HOPS * bounce::HOP_SECONDS));
            }
            if changed {
                self.sync_badge_visuals()?;
                self.render()?;
            }
            // HSHELL_FLASH is only an attention request: it carries neither a
            // notification body nor an unread count. Repeated flashes must not
            // manufacture banners or increment message counters.
        }
        Ok(())
    }

    pub(super) fn sync_badge_visuals(&mut self) -> Result<()> {
        // Windows' own switch for taskbar badges ("Show badges on taskbar
        // apps"), which Nosih follows as the taskbar does.
        let shown = self.attention_enabled;
        crate::overlays::retain(|hwnd| self.windows.iter().any(|window| window.hwnd == hwnd));
        // An app's own overlay says more than the attention dot: it replaces it.
        let overlays: Vec<_> = self
            .entries
            .iter()
            .map(|entry| shown.then(|| entry_overlay(&self.windows, entry)).flatten())
            .collect();
        if let Some(stage) = &mut self.stage {
            stage
                .scene
                .set_overlays(&self.gfx, overlays.iter().cloned())?;
            stage.scene.set_attention(
                &self.gfx,
                self.entries.iter().zip(&overlays).map(|(entry, overlay)| {
                    shown
                        && overlay.is_none()
                        && entry_has_attention(&self.attention, &self.windows, entry)
                }),
            )?;
        }
        if let Some(view) = &mut self.group_view {
            view.set_attention(&self.gfx, |id| {
                self.attention_enabled && app_has_attention(&self.attention, &self.windows, id)
            })?;
        }
        Ok(())
    }

    pub(super) fn taskbar_preferences_changed(&mut self) -> Result<()> {
        if let Some(watch) = &mut self.badge_watch {
            watch.arm();
        }
        let enabled = status::taskbar_badges_enabled();
        if self.attention_enabled != enabled {
            self.attention_enabled = enabled;
            self.attention.clear();
            self.sync_taskband();
            self.sync_badge_visuals()?;
            self.render()?;
        }
        Ok(())
    }

    pub(super) fn forget_attention(&mut self, hwnd: isize) -> Result<()> {
        if self.attention.acknowledge(hwnd) {
            self.sync_badge_visuals()?;
            self.render()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod attention_tests {
    use super::super::{HSHELL_FLASH, window_id};
    use super::*;
    use nosih_core::layout::ItemKind;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            RegisterShellHookWindow, RegisterWindowMessageW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WS_POPUP,
        },
        core::w,
    };

    #[test]
    fn attention_reaches_apps_groups_and_minimized_thumbnails_without_clearing_other_windows() {
        let windows = [
            TrackedWindow {
                hwnd: 7,
                app_id: "chat".into(),
                minimized: true,
            },
            TrackedWindow {
                hwnd: 8,
                app_id: "chat".into(),
                minimized: false,
            },
        ];
        let mut attention = Attention::default();
        attention.request(7, 2);
        let app = Entry {
            kind: ItemKind::Icon,
            id: Some("chat".into()),
            role: Role::App,
            running: true,
            members: Vec::new(),
        };
        let group = Entry {
            id: Some("group:chat".into()),
            role: Role::Group,
            members: vec!["chat".into()],
            ..app.clone()
        };
        let thumbnail = Entry {
            id: Some(window_id(7)),
            role: Role::Window,
            ..app.clone()
        };
        assert!(entry_has_attention(&attention, &windows, &app));
        assert!(entry_has_attention(&attention, &windows, &group));
        assert!(entry_has_attention(&attention, &windows, &thumbnail));
        assert_eq!(active_attention_window(&windows, 7), 0);
        attention.acknowledge(active_attention_window(&windows, 8));
        assert!(entry_has_attention(&attention, &windows, &app));
        attention.acknowledge(7);
        assert!(!entry_has_attention(&attention, &windows, &group));
    }

    #[test]
    #[ignore = "Requires Explorer's shell hooks; uses only off-screen test-owned windows"]
    fn native_flash_request_produces_the_shell_event_used_by_dock_badges() -> Result<()> {
        use std::time::Duration;
        use windows::Win32::UI::WindowsAndMessaging::{
            DeregisterShellHookWindow, DestroyWindow, FLASHW_STOP, FLASHW_TRAY, FLASHWINFO,
            FlashWindowEx, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
        };
        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    let _ = DeregisterShellHookWindow(self.0);
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let receiver = Window(unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Nosih badge receiver test"),
                WS_POPUP,
                -32000,
                -32000,
                80,
                40,
                None,
                None,
                None,
                None,
            )?
        });
        let source = Window(unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("Nosih native attention test"),
                WS_OVERLAPPEDWINDOW,
                -32000,
                -32000,
                160,
                120,
                None,
                None,
                None,
                None,
            )?
        });
        let message = unsafe {
            RegisterShellHookWindow(receiver.0).ok()?;
            let _ = ShowWindow(source.0, SW_SHOWNOACTIVATE);
            RegisterWindowMessageW(w!("SHELLHOOK"))
        };
        let pump = |duration: Duration| {
            let until = Instant::now() + duration;
            let mut found = false;
            while Instant::now() < until {
                let mut event = MSG::default();
                while unsafe { PeekMessageW(&mut event, None, 0, 0, PM_REMOVE) }.as_bool() {
                    found |= event.hwnd == receiver.0
                        && event.message == message
                        && event.wParam.0 == HSHELL_FLASH
                        && event.lParam.0 == source.0.0 as isize;
                    unsafe {
                        DispatchMessageW(&event);
                    }
                }
                if found {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            found
        };
        pump(Duration::from_millis(350));
        let mut info = FLASHWINFO {
            cbSize: size_of::<FLASHWINFO>() as u32,
            hwnd: source.0,
            dwFlags: FLASHW_TRAY,
            uCount: 2,
            dwTimeout: 100,
        };
        unsafe {
            let _ = FlashWindowEx(&info);
        }
        let found = pump(Duration::from_secs(2));
        info.dwFlags = FLASHW_STOP;
        unsafe {
            let _ = FlashWindowEx(&info);
        }
        assert!(found, "Explorer did not publish the native attention event");
        Ok(())
    }
}

#[cfg(test)]
mod performance_tests {
    use super::super::BinQueryGate;

    #[test]
    fn recycle_notifications_share_one_scan_and_preserve_the_last_change() {
        let gate = BinQueryGate::new();
        assert!(gate.request());
        gate.begin_pass();
        for _ in 0..10_000 {
            assert!(!gate.request());
        }
        assert!(gate.needs_another_pass());
        gate.begin_pass();
        assert!(!gate.needs_another_pass());
        assert!(gate.request());
    }

    #[test]
    fn quiet_recycle_scan_allows_the_next_independent_scan() {
        let gate = BinQueryGate::new();
        assert!(gate.request());
        gate.begin_pass();
        assert!(!gate.needs_another_pass());
        assert!(gate.request());
    }
}

#[cfg(test)]
mod mascot_power_tests {
    use super::{mascot::Act, power_act};

    #[test]
    fn initial_power_state_and_repeated_notifications_do_not_play_an_act() {
        assert_eq!(power_act(None, (Some(true), true), true), None);
        assert_eq!(
            power_act(Some((Some(false), true)), (Some(false), true), true),
            None
        );
        assert_eq!(
            power_act(Some((Some(true), false)), (Some(true), false), true),
            None
        );
    }

    #[test]
    fn saver_entry_and_charging_have_distinct_acts() {
        assert_eq!(
            power_act(Some((Some(false), false)), (Some(false), true), true),
            Some(Act::Sleepy)
        );
        assert_eq!(
            power_act(Some((Some(false), false)), (Some(true), false), true),
            Some(Act::Charging)
        );
        assert_eq!(
            power_act(Some((Some(false), false)), (Some(true), true), true),
            Some(Act::Sleepy)
        );
    }

    #[test]
    fn charging_cooldown_and_unknown_battery_state_suppress_false_plug_events() {
        assert_eq!(
            power_act(Some((Some(false), false)), (Some(true), false), false),
            None
        );
        assert_eq!(
            power_act(Some((None, false)), (Some(true), false), true),
            None
        );
        assert_eq!(
            power_act(Some((Some(true), true)), (Some(false), false), true),
            Some(Act::Unplugged)
        );
    }

    #[test]
    fn unplugging_is_not_suppressed_by_charging_cooldown_or_automatic_saver() {
        assert_eq!(
            power_act(Some((Some(true), false)), (Some(false), true), false),
            Some(Act::Unplugged)
        );
        assert_eq!(
            power_act(Some((Some(true), false)), (Some(false), false), false),
            Some(Act::Unplugged)
        );
    }

    #[test]
    fn missing_or_repeated_power_readings_do_not_announce_a_disconnect() {
        for (before, now) in [
            (None, Some(false)),
            (Some(true), None),
            (Some(false), Some(false)),
        ] {
            assert_eq!(power_act(Some((before, false)), (now, false), true), None);
        }
    }
}
