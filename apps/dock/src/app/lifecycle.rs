//! Startup, shutdown, taskbar takeover, updates and the message loop.

use std::{collections::HashMap, env, sync::atomic::Ordering, thread, time::Instant};

use anyhow::Result;
use nosih_core::{
    attention::Attention,
    frame::DockEdge,
    spring::{ElasticSpring, Spring},
};
use nosih_shell::{
    appbar::AppBar,
    power,
    recycle_bin::BinWatch,
    status,
    system::downloads_folder,
    taskbar,
    tracker::{self, WindowTracker},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmFlush, DwmSetWindowAttribute},
        System::{
            Console::{CTRL_C_EVENT, SetConsoleCtrlHandler},
            LibraryLoader::GetModuleHandleW,
            Ole::{IDropTarget, OleInitialize, OleUninitialize, RegisterDragDrop, RevokeDragDrop},
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU, VK_TAB},
            WindowsAndMessaging::{
                CreateWindowExW, DispatchMessageW, EVENT_OBJECT_CLOAKED,
                EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MOVESIZEEND,
                FindWindowW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PM_REMOVE,
                PeekMessageW, PostMessageW, RegisterClassW, RegisterShellHookWindow,
                TranslateMessage,
                RegisterWindowMessageW, SetTimer, WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT,
                WINEVENT_SKIPOWNPROCESS, WM_CLOSE, WM_QUIT, WNDCLASSW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{BOOL, Interface, w},
};

use crate::{assets::Visuals, drop::DropTarget, gfx::Gfx, strings, thumbnails::Thumbnails};

use super::{
    APP, APPBAR_CALLBACK, ASSETS_READY, App, BIN_CHANGED, CLOAK_HOOK, CONFIG_CHANGED, DROP_WINDOW,
    FOREGROUND_HOOK, CURSOR_DAMPING, CURSOR_OMEGA, HOVER_DAMPING, HOVER_OMEGA, PRESS_DAMPING, PRESS_OMEGA, LAST_FRONT, LAUNCH_BOUNCE_LIMIT, LOCATION_HOOK, MAIN_WINDOW,
    MAX_SNAPSHOTS, MINIMIZE_HOOK, MOVESIZE_HOOK, Outside, PINS_CHANGED, PreviewOrder, SHELL_HOOK,
    SYNC_DELAY_MS, SYNC_TIMER, TASKBAR_CREATED, TASKBAR_DELAY_MS, TASKBAR_MAX_ATTEMPTS,
    TASKBAR_PREFS_CHANGED, TASKBAR_TAKEN, TASKBAR_TIMER, UPDATE_AFTER_START, UPDATE_EVERY,
    UPDATE_RETRY, UPDATE_TIMER, WINDOW_CLASS,
    events::{on_win_event, wndproc},
    feedback::query_bin_in_background,
    millis,
    preferences::{load_config, roster_from, save_settings},
    with_app, with_app_then,
};

pub fn request_quit() {
    // SAFETY: Searches for the window and posts a message with no associated data.
    unsafe {
        if let Ok(hwnd) = FindWindowW(WINDOW_CLASS, None) {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub(super) unsafe extern "system" fn on_console_ctrl(event: u32) -> BOOL {
    let hwnd = MAIN_WINDOW.load(Ordering::Relaxed);
    if event == CTRL_C_EVENT && hwnd != 0 {
        // SAFETY: Only a message is sent; if the window no longer exists, the call has no effect.
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return true.into();
    }
    false.into()
}

pub(super) fn main_window() -> HWND {
    HWND(MAIN_WINDOW.load(Ordering::Relaxed) as *mut _)
}

/// `true` if the dock already lists the window `hwnd`.
pub(super) fn is_tracked(hwnd: isize) -> bool {
    APP.with(|cell| {
        cell.try_borrow().is_ok_and(|app| {
            app.as_ref()
                .is_some_and(|app| app.windows.iter().any(|window| window.hwnd == hwnd))
        })
    })
}

pub(super) fn is_animating() -> bool {
    APP.with_borrow(|app| app.as_ref().is_some_and(App::is_animating))
}

/// Main loop. Sleeps while idle (0% CPU) and advances one frame per display
/// refresh while an animation is running.
///
/// Outside animations, the thread uses EcoQoS because remaining work (window
/// tracking and recomposition) is not time-sensitive.
pub(super) fn message_loop() {
    let mut msg = MSG::default();
    let mut was_animating = None;
    unsafe {
        loop {
            let animating = is_animating();
            if was_animating != Some(animating) {
                power::set_thread_efficiency(!animating);
                if was_animating == Some(true) {
                    with_app("recorte de memoria", |app| {
                        app.schedule_trim();
                        Ok(())
                    });
                }
                was_animating = Some(animating);
            }
            if animating {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                with_app_then("animación", App::tick);
                // Wait for the next refresh to pace animation without timers.
                let _ = DwmFlush();
            } else {
                let status = GetMessageW(&mut msg, None, 0, 0);
                if status.0 <= 0 {
                    if status.0 < 0 {
                        eprintln!("nosih: error al leer la cola de mensajes de Windows");
                    }
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

pub fn run() -> Result<()> {
    let startup = Instant::now();
    nosih_shell::log::line(concat!("inicio, versión ", env!("CARGO_PKG_VERSION")));
    let gfx = Gfx::new()?.with_error_handler(crate::recovery::graphics_error);
    // SAFETY: The thread is already a single-threaded COM apartment (see
    // `main`); OLE adds drag and drop on top of it and is released at the end.
    unsafe { OleInitialize(None)? };

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WINDOW_CLASS;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        });

        let base = WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        let create = |extra: WINDOW_EX_STYLE| {
            CreateWindowExW(
                base | extra,
                class,
                w!("Nosih"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        };
        let visual = create(WS_EX_LAYERED | WS_EX_TRANSPARENT)?;
        let input = create(WINDOW_EX_STYLE::default())?;

        // Without this, the system backdrop brush appears black. Windows 10
        // does not know the attribute; the dock then draws solid glass
        // (`system::transparency_enabled`), so the error is not fatal.
        let enable = BOOL::from(true);
        let _ = DwmSetWindowAttribute(
            visual,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            (&raw const enable).cast(),
            size_of::<BOOL>() as u32,
        );

        let target = gfx
            .compositor
            .cast::<ICompositorDesktopInterop>()?
            .CreateDesktopWindowTarget(visual, true)?;

        nosih_config::migrate_legacy_folder();
        let config = load_config();
        strings::set_language(config.language);
        crate::appearance::set(config.appearance);
        crate::appearance::set_menu_bar(config.menu_bar_appearance);
        let roster = roster_from(&config);
        let auto_hide = config.auto_hide;
        let slide_omega = config.dock_auto_hide_speed.omega();
        let mut app = App {
            visual,
            input,
            gfx,
            target,
            appbar: AppBar::register(visual, APPBAR_CALLBACK),
            taskbar_worker: None,
            taskbar_retry: false,
            taskbar_attempts: 0,
            fullscreen: false,
            smart_hidden: false,
            roster,
            tracker: WindowTracker::default(),
            windows: Vec::new(),
            preview_order: PreviewOrder::default(),
            last_rects: HashMap::new(),
            entries: Vec::new(),
            icons: Visuals::new(visual, ASSETS_READY)?,
            downloads: downloads_folder(),
            bin_full: false,
            _bin_watch: BinWatch::register(input, BIN_CHANGED),
            _saver_watch: power::SaverWatch::new(input.0 as isize),
            pin_watch: nosih_shell::pins::watch(visual.0 as isize, PINS_CHANGED),
            badge_watch: status::watch_taskbar_settings(visual.0 as isize, TASKBAR_PREFS_CHANGED),
            attention: Attention::default(),
            attention_enabled: status::taskbar_badges_enabled(),
            assets: HashMap::new(),
            assets_for: (0.0, false, 0, DockEdge::Bottom),
            placeholder: None,
            remove_label: None,
            config,
            slide: Spring::at(if auto_hide { 1.0 } else { 0.0 }, slide_omega),
            press: None,
            drag: None,
            incoming: Vec::new(),
            incoming_pin: None,
            drag_over: false,
            spring: None,
            spring_opened_group: false,
            switching: false,
            switch_started: Instant::now(),
            switch_ended: None,
            launched: Instant::now(),
            update_tried: None,
            follow: None,
            pointed: false,
            pending_drop: None,
            bounces: HashMap::new(),
            minimized: Vec::new(),
            thumbnails: Thumbnails::new(visual),
            flights: Vec::new(),
            snapshots: Vec::new(),
            menubar: None,
            spotlight: None,
            hud: None,
            edge_watch: None,
            dock_display: None,
            names: HashMap::new(),
            calmed: Vec::new(),
            started: false,
            stage: None,
            hovering: false,
            hovered: None,
            preview_candidate: None,
            preview: None,
            group_view: None,
            group_was_open: None,
            ghost: None,
            menu_open: false,
            mascot: None,
            // Initial registration notifications establish the baseline after
            // the queue starts, without playing a startup charging/saver act.
            power_seen: None,
            charging_shown: None,
            moments: crate::moments::Moments::default(),
            welcome_tries: 0,
            cursor: 0.0,
            cursor_spring: ElasticSpring::at(0.0, CURSOR_OMEGA, CURSOR_DAMPING),
            amount: ElasticSpring::at(0.0, HOVER_OMEGA, HOVER_DAMPING),
            press_scale: ElasticSpring::at(1.0, PRESS_OMEGA, PRESS_DAMPING),
            press_index: None,
            last_frame: Instant::now(),
        };
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let _ = CONFIG_CHANGED.set(RegisterWindowMessageW(
            nosih_shell::dock_link::CONFIG_CHANGED,
        ));
        MAIN_WINDOW.store(visual.0 as isize, Ordering::Relaxed);
        // The dock inherits the "ignore Ctrl+C" attribute from its parent
        // (some tools enable it), so clear it to ensure Ctrl+C is handled.
        let _ = SetConsoleCtrlHandler(None, false);
        let _ = SetConsoleCtrlHandler(Some(on_console_ctrl), true);

        // Apps uninstalled while Nosih was closed leave the dock.
        if app.prune_missing_apps() {
            save_settings(&app.roster, &app.config);
        }
        // Pins and unpins made on the Windows taskbar while Nosih was closed.
        if let Err(error) = app.sync_taskbar_pins() {
            eprintln!("nosih: apps fijadas de la barra de Windows: {error:#}");
        }
        app.sync()?;
        // `sync` recomposes only when the list changes; run it unconditionally at startup.
        if app.stage.is_none() {
            app.refresh()?;
        }
        // After the dock: creating the bar and reserving its strip take
        // 60-130 ms, which the dock no longer waits for.
        app.apply_menu_bar_setting();
        // Otherwise the bar shows no app name until the user switches windows.
        if let Err(error) = app.update_menu_bar_app() {
            eprintln!("nosih: barra superior: {error:#}");
        }

        // The shell notifies this window when a window is created, closed,
        // activated, or requests attention.
        let _ = SHELL_HOOK.set(RegisterWindowMessageW(w!("SHELLHOOK")));
        RegisterShellHookWindow(input).ok()?;
        // Delivered through this thread's message loop; no code runs in other processes.
        let minimize_hook = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        MINIMIZE_HOOK.store(minimize_hook.0 as isize, Ordering::Relaxed);
        // Switching virtual desktops hides (cloaks) the windows of the one
        // left and shows those of the one entered, and no shell hook says so.
        let cloak_hook = SetWinEventHook(
            EVENT_OBJECT_CLOAKED,
            EVENT_OBJECT_UNCLOAKED,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        CLOAK_HOOK.store(cloak_hook.0 as isize, Ordering::Relaxed);
        // Alt+Tab: the switcher coming to the front. One event per window
        // switch, so it costs nothing while the user works in one app.
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        FOREGROUND_HOOK.store(foreground_hook.0 as isize, Ordering::Relaxed);
        let location_hook = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        LOCATION_HOOK.store(location_hook.0 as isize, Ordering::Relaxed);
        let movesize_hook = SetWinEventHook(
            EVENT_SYSTEM_MOVESIZEEND,
            EVENT_SYSTEM_MOVESIZEEND,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        MOVESIZE_HOOK.store(movesize_hook.0 as isize, Ordering::Relaxed);
        LAST_FRONT.store(tracker::foreground_window(), Ordering::Relaxed);
        let target: IDropTarget = DropTarget.into();
        RegisterDragDrop(input, &target)?;
        DROP_WINDOW.store(input.0 as isize, Ordering::Relaxed);

        app.schedule_update();
        app.sync_break_timer();
        app.sync_taskband();
        // The day's greeting, or a new version's celebration; and again
        // after an unlock (see `on_session_change`).
        app.arm_welcome();
        let _ = WTSRegisterSessionNotification(input, NOTIFY_FOR_THIS_SESSION);
        APP.set(Some(app));
    }

    // Hiding the taskbar is a synchronous Explorer call that takes 50–500 ms
    // depending on load. Run it off the UI thread after the dock is visible.
    query_bin_in_background();
    with_app("ocultar la barra de Windows", |app| {
        app.request_taskbar(false)
    });

    nosih_shell::log::line(&format!(
        "interfaz inicializada en {} ms desde app::run; comienza el bucle de mensajes",
        startup.elapsed().as_millis()
    ));
    message_loop();
    // The taskbar's overlay calls go back to Explorer.
    crate::overlays::stop();
    // SAFETY: The drop target was registered on this window in this thread.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(
            Some(main_window()),
            super::SPOTLIGHT_HOTKEY_ID,
        );
        let _ = RevokeDragDrop(HWND(DROP_WINDOW.load(Ordering::Relaxed) as *mut _));
        let _ =
            WTSUnRegisterSessionNotification(HWND(DROP_WINDOW.load(Ordering::Relaxed) as *mut _));
        for hook in [
            &MINIMIZE_HOOK,
            &CLOAK_HOOK,
            &FOREGROUND_HOOK,
            &LOCATION_HOOK,
            &MOVESIZE_HOOK,
        ] {
            let _ = UnhookWinEvent(HWINEVENTHOOK(hook.load(Ordering::Relaxed) as *mut _));
        }
    }
    let taking_over = APP.with_borrow_mut(|app| {
        app.as_mut().and_then(|app| {
            app.restore_transitions();
            app.taskbar_worker.take()
        })
    });
    APP.set(None);
    // SAFETY: Balances `OleInitialize` above, on the same thread.
    unsafe { OleUninitialize() };
    // Wait for taskbar takeover to finish before restoring it.
    if let Some(worker) = taking_over {
        let _ = worker.join();
    }
    Ok(())
}

impl App {
    /// Aims the update timer at the next look: a day after the last one,
    /// which survives restarts, and never sooner than two minutes after the
    /// dock starts. So the dock wakes for it once a day, not on a schedule.
    pub(super) fn schedule_update(&self) {
        // SAFETY: Timers on our own window; setting one again replaces it.
        unsafe {
            if !self.config.check_updates || !self.config.auto_update {
                let _ = KillTimer(Some(self.input), UPDATE_TIMER);
                return;
            }
            // Also counted from the last try in this session: if the date
            // could not be saved, the registry would call it due again at
            // once, and the dock would start one update after another.
            let since_try = self
                .update_tried
                .map_or(std::time::Duration::ZERO, |tried| {
                    UPDATE_EVERY.saturating_sub(tried.elapsed())
                });
            let wait = nosih_shell::updates::due_in(UPDATE_EVERY)
                .max(UPDATE_AFTER_START.saturating_sub(self.launched.elapsed()))
                .max(since_try);
            SetTimer(Some(self.input), UPDATE_TIMER, millis(wait), None);
        }
    }

    /// Starts the background update: Settings without a window, which does
    /// the network part and exits when there is nothing to install. The
    /// dock itself never touches the network.
    pub(super) fn update_now(&mut self) {
        // SAFETY: Our own timer.
        unsafe {
            let _ = KillTimer(Some(self.input), UPDATE_TIMER);
        }
        if !self.config.check_updates || !self.config.auto_update {
            return;
        }
        // Installing restarts the dock: not in the middle of a game.
        if self.fullscreen {
            // SAFETY: Our own timer.
            unsafe { SetTimer(Some(self.input), UPDATE_TIMER, millis(UPDATE_RETRY), None) };
            return;
        }
        // Recorded first: whatever happens, the next look is tomorrow.
        nosih_shell::updates::mark_checked();
        self.update_tried = Some(Instant::now());
        self.schedule_update();
        let settings = env::current_exe()
            .ok()
            .map(|exe| exe.with_file_name("nosih-settings.exe"))
            .filter(|path| path.exists());
        if let Some(settings) = settings
            && let Err(error) = std::process::Command::new(settings).arg("--update").spawn()
        {
            eprintln!("nosih: actualización: {error}");
        }
    }

    pub(super) fn tick(&mut self) -> Result<Option<Outside>> {
        let now = Instant::now();
        let flying = self.flights.len();
        let gfx = &self.gfx;
        let snapshots = &mut self.snapshots;
        let mut restore = None;
        let foreground = tracker::foreground_window();
        // The Alt+Tab switcher can briefly have no foreground HWND. Cancel
        // before a deferred restore gets a chance to fight the keyboard.
        let switching = unsafe {
            GetAsyncKeyState(i32::from(VK_MENU.0)) < 0 && GetAsyncKeyState(i32::from(VK_TAB.0)) < 0
        };
        self.flights.retain_mut(|flight| {
            if switching || !flight.is_current(foreground) {
                return false;
            }
            let going = flight.step(gfx, now);
            if flight.take_restore(!going) {
                restore = Some(flight.hwnd);
            }
            if !going && let Some(snapshot) = flight.take_snapshot_for_restore() {
                if snapshots.len() >= MAX_SNAPSHOTS {
                    snapshots.remove(0);
                }
                snapshots.push((flight.hwnd, snapshot));
            }
            going
        });
        if self.flights.len() != flying {
            // Confirm the windows really are minimized, or restored.
            // SAFETY: Timer on our own window.
            unsafe { SetTimer(Some(self.input), SYNC_TIMER, SYNC_DELAY_MS, None) };
        }
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.amount.step(dt);
        self.cursor_spring.step(dt);
        self.press_scale.step(dt);
        if self.press.is_none() && self.press_scale.is_settled() {
            self.press_index = None;
        }
        self.slide.step(dt);
        self.last_frame = now;

        self.bounces.retain(|_, bounce| {
            let elapsed = now.duration_since(bounce.start).as_secs_f32();
            let until = bounce.until.unwrap_or(LAUNCH_BOUNCE_LIMIT);
            elapsed < until
        });
        self.render()?;
        Ok(restore.map(|hwnd| Outside::Activate(vec![hwnd])))
    }

    /// Explorer can block on taking over the taskbar, especially after it
    /// restarts. Coalesce notifications while the worker owns that request.
    pub(super) fn request_taskbar(&mut self, rebuild: bool) -> Result<()> {
        if self.taskbar_worker.is_some() {
            self.taskbar_retry |= rebuild;
            return Ok(());
        }
        let window = self.visual.0 as isize;
        self.taskbar_attempts = self.taskbar_attempts.saturating_add(1);
        self.taskbar_worker = Some(thread::Builder::new().name("nosih-taskbar".into()).spawn(
            move || {
                let hidden = taskbar::take_over();
                // SAFETY: Only a message with scalar data is sent to our UI.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(window as *mut _)),
                        TASKBAR_TAKEN,
                        WPARAM(usize::from(rebuild)),
                        LPARAM(isize::from(hidden)),
                    );
                }
            },
        )?);
        Ok(())
    }

    pub(super) fn taskbar_taken(&mut self, rebuild: bool, hidden: bool) -> Result<()> {
        // The worker has finished its Explorer call and posted this reply.
        self.taskbar_worker = None;
        if std::mem::take(&mut self.taskbar_retry) {
            return self.request_taskbar(true);
        }
        if !hidden {
            // Explorer is still starting or recreated its taskbar during our
            // request. Retry briefly without blocking the UI or polling forever.
            if self.taskbar_attempts < TASKBAR_MAX_ATTEMPTS {
                unsafe {
                    SetTimer(Some(self.input), TASKBAR_TIMER, TASKBAR_DELAY_MS, None);
                }
            } else {
                nosih_shell::log::line(
                    "barra de Windows pendiente tras 50 intentos; se reintentará al reiniciarse Explorer o reanudar Windows",
                );
            }
            return Ok(());
        }
        self.taskbar_attempts = 0;
        if rebuild {
            self.appbar = None;
            self.appbar = AppBar::register(self.visual, APPBAR_CALLBACK);
            self.menubar = None;
            self.apply_menu_bar_setting();
        }
        self.stage = None;
        self.refresh()
    }

    pub(super) fn is_animating(&self) -> bool {
        !self.amount.is_settled()
            || !self.cursor_spring.is_settled()
            || !self.press_scale.is_settled()
            || !self.slide.is_settled()
            || self
                .stage
                .as_ref()
                .is_some_and(|stage| stage.reorder.is_some() || stage.scene.is_glint_animating())
            || !self.bounces.is_empty()
            || !self.flights.is_empty()
    }
}
