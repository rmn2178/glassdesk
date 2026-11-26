//! Win32 callbacks and popup/OLE entry points forwarding into the UI state.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use nosih_shell::{launcher, power, taskbar, tracker};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::Ole::DROPEFFECT,
    UI::{
        Accessibility::HWINEVENTHOOK,
        Input::KeyboardAndMouse::GetCapture,
        WindowsAndMessaging::{
            DefWindowProcW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
            EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MOVESIZEEND, HMENU, KillTimer, MA_ACTIVATE,
            MA_NOACTIVATE, OBJID_WINDOW, PostMessageW, PostQuitMessage, SPI_SETDESKWALLPAPER,
            SetTimer, WM_CAPTURECHANGED, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
            WM_DWMCOLORIZATIONCOLORCHANGED, WM_INITMENUPOPUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP,
            WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK,
        },
    },
};

use crate::{edge_watch, folder_menu, multi_window_view::MultiWindowAction};

use super::{
    APPBAR_CALLBACK, ASSETS_DELAY_MS, ASSETS_READY, ASSETS_TIMER, App, BIN_CHANGED, BIN_DELAY_MS,
    BIN_STATE, BIN_TIMER, BREAK_TIMER, CONFIG_CHANGED, DRAG_CANCELLED, DRAG_TIMER, DROP_RUN,
    EDGE_TIMER, FULLSCREEN_TIMER, HEALTH_CHECK, HIDE_TIMER, HSHELL_FLASH, HSHELL_LANGUAGE,
    HSHELL_REDRAW, HSHELL_RUDEAPPACTIVATED, HSHELL_WINDOWACTIVATED, HSHELL_WINDOWDESTROYED,
    LAST_FRONT, MASCOT_TIMER, Outside, PBT_APMPOWERSTATUSCHANGE, PBT_APMRESUMEAUTOMATIC,
    PBT_POWERSETTINGCHANGE, PINS_CHANGED, PINS_DELAY_MS, PINS_TIMER, PREVIEW_TIMER,
    REFRESH_DELAY_MS, REFRESH_TIMER, SHELL_HOOK, SPI_SETWORKAREA, SPRING_TIMER, SWITCHER_TIMER,
    SYNC_DELAY_MS, SYNC_TIMER, TASKBAR_CREATED, TASKBAR_DELAY_MS, TASKBAR_PREFS_CHANGED,
    TASKBAR_TAKEN, TASKBAR_TIMER, TRIM_TIMER, UPDATE_TIMER, WELCOME_TIMER, WM_MOUSELEAVE,
    WM_POWERBROADCAST,
    feedback::query_bin_in_background,
    lifecycle::{is_tracked, main_window},
    with_app, with_app_then,
};

const BACKDROP_REFRESH: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
static BACKDROP_PENDING: AtomicBool = AtomicBool::new(false);

fn queue_backdrop_refresh(hwnd: HWND) {
    if hwnd.0.is_null() || BACKDROP_PENDING.swap(true, Ordering::Relaxed) {
        return;
    }
    // SAFETY: Only a payload-free notification is queued, never a synchronous
    // call into another app. Both dock HWNDs coalesce onto the main UI window.
    if unsafe { PostMessageW(Some(hwnd), BACKDROP_REFRESH, WPARAM(0), LPARAM(0)) }.is_err() {
        BACKDROP_PENDING.store(false, Ordering::Relaxed);
    }
}

pub(super) unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW.0 {
        return;
    }
    if event == EVENT_SYSTEM_MINIMIZESTART {
        with_app("minimizar", |app| app.on_minimize_start(hwnd.0 as isize));
    } else if event == EVENT_SYSTEM_MINIMIZEEND {
        // Restored from outside the dock (Alt+Tab, the app itself): no shell
        // hook reports it, and a window the dock still thought minimized kept
        // its thumbnail and skipped the genie on its next minimize.
        with_app("restaurar", App::sync);
    } else if event == EVENT_SYSTEM_FOREGROUND {
        if tracker::is_task_switcher(hwnd) {
            with_app("Alt+Tab", App::task_switcher_opened);
        } else if taskbar::is_hidden(hwnd) && !launcher::opening_hidden_icons() {
            with_app("barra de tareas oculta", App::leave_hidden_taskbar);
        } else if !tracker::is_switch_transient(hwnd) {
            LAST_FRONT.store(hwnd.0 as isize, Ordering::Relaxed);
            with_app("ventana activa", App::on_foreground_changed);
        }
    } else if event == EVENT_OBJECT_LOCATIONCHANGE {
        // Only the foreground top-level window matters. Ignore child controls,
        // carets and background animations, and coalesce resize bursts using
        // the existing full-screen timer, without enumerating apps or polling.
        if child == 0 && hwnd.0 as isize == tracker::foreground_window() {
            unsafe { SetTimer(Some(main_window()), FULLSCREEN_TIMER, SYNC_DELAY_MS, None) };
        }
    } else if event == EVENT_SYSTEM_MOVESIZEEND {
        // A window let go of after a drag, even one that did not move at the
        // end: the same re-check, which keeps its title bar out from under
        // the menu bar (issue #53). One event per drag.
        // SAFETY: A timer on our own window.
        unsafe { SetTimer(Some(main_window()), FULLSCREEN_TIMER, SYNC_DELAY_MS, None) };
    } else if event == EVENT_OBJECT_CLOAKED || event == EVENT_OBJECT_UNCLOAKED {
        // Shows the windows of the virtual desktop in front, as the Windows
        // taskbar does. A desktop switch cloaks many windows at once; the
        // timer turns the burst into one re-read.
        // SAFETY: A timer on our own window.
        unsafe {
            SetTimer(Some(main_window()), SYNC_TIMER, SYNC_DELAY_MS, None);
        }
    }
}

/// Like `with_app`, for callers that need a value back (OLE callbacks).
pub(super) fn with_app_value<T: Default>(
    what: &str,
    action: impl FnOnce(&mut App) -> Result<T>,
) -> T {
    let mut value = T::default();
    with_app(what, |app| {
        value = action(app)?;
        Ok(())
    });
    value
}

pub fn menu_bar_clicked(x: f32, y: f32) {
    with_app_then("barra superior", |app| {
        if let Some(bar) = &mut app.menubar {
            if let Some(anchor) = bar.input_switcher_anchor(x, y) {
                bar.dismiss_island(&app.gfx)?;
                return Ok(Some(Outside::InputMenu {
                    gfx: app.gfx.clone(),
                    anchor,
                    dark: !crate::appearance::menu_bar_light(),
                }));
            }
            bar.click(&app.gfx, x, y)?;
        }
        Ok(None)
    });
}

pub fn menu_bar_volume_press(x: f32, y: f32) {
    with_app("volumen", |app| {
        if let Some(bar) = &mut app.menubar {
            bar.press_volume(&app.gfx, x, y)?;
        }
        Ok(())
    });
}

pub fn menu_bar_scrolled(point: POINT, delta: i16) -> bool {
    with_app_value("desplazar uso de IA", |app| match &mut app.menubar {
        Some(bar) => bar.scroll_usage_at(point, delta),
        None => Ok(false),
    })
}

pub fn menu_bar_volume_release(point: Option<(f32, f32)>) -> bool {
    with_app_value("volumen", |app| match &mut app.menubar {
        Some(bar) => bar.release_volume(point),
        None => Ok(false),
    })
}

pub fn menu_bar_volume_flush() {
    with_app("volumen", |app| match &mut app.menubar {
        Some(bar) => bar.flush_volume(),
        None => Ok(()),
    });
}

pub fn menu_bar_brightness_flush() {
    with_app("brillo", |app| match &mut app.menubar {
        Some(bar) => bar.flush_brightness(),
        None => Ok(()),
    });
}

pub fn menu_bar_control_center_changed() {
    with_app("centro de control", |app| match &mut app.menubar {
        Some(bar) => bar.control_center_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_notifications_changed() {
    with_app("notificaciones", |app| match &mut app.menubar {
        Some(bar) => bar.notifications_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_notification_banner_timeout() {
    with_app("banner de notificación", |app| match &mut app.menubar {
        Some(bar) => bar.notification_banner_timeout(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_progress_tick() {
    with_app("progreso multimedia", |app| match &mut app.menubar {
        Some(bar) => bar.progress_tick(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_monitor_tick() {
    with_app("monitor del sistema", |app| match &mut app.menubar {
        Some(bar) => bar.monitor_tick(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_media_changed() {
    with_app("reproducción", |app| match &mut app.menubar {
        Some(bar) => bar.media_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_audio_changed() {
    with_app("audio", |app| match &mut app.menubar {
        Some(bar) => bar.audio_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_bluetooth_changed() {
    with_app("Bluetooth", |app| match &mut app.menubar {
        Some(bar) => bar.bluetooth_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_accessories_changed() {
    with_app("accesorios", |app| match &mut app.menubar {
        Some(bar) => bar.accessories_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_accessories_reset() {
    with_app("reanudar accesorios", |app| match &mut app.menubar {
        Some(bar) => bar.restart_accessories(&app.gfx),
        None => Ok(()),
    });
}

pub fn multi_window_view_tick() {
    with_app("vistas de ventanas", |app| {
        if let Some(view) = &mut app.preview
            && !view.tick()?
        {
            app.close_preview();
            app.render()?;
        }
        Ok(())
    });
}

pub fn multi_window_view_press() {
    with_app("seleccionar ventana", |app| {
        if let Some(view) = &mut app.preview {
            view.press()?;
        }
        Ok(())
    });
}

pub fn multi_window_view_release() {
    with_app_then("abrir ventana", |app| {
        let action = match &mut app.preview {
            Some(view) => view.release()?,
            None => None,
        };
        match action {
            Some(MultiWindowAction::Activate(hwnd)) => {
                app.close_preview();
                // Resolve against the current tracker snapshot: a closed window
                // must never select a different entry by its former index.
                if app.windows.iter().any(|window| window.hwnd == hwnd) {
                    return Ok(app.activating(vec![hwnd]));
                }
            }
            Some(MultiWindowAction::Close(hwnd)) => {
                nosih_shell::tracker::request_close_window(hwnd);
                if let Some(view) = &mut app.preview {
                    let still_open = view.remove_window(&app.gfx, hwnd)?;
                    if !still_open {
                        app.close_preview();
                    }
                }
            }
            None => {}
        }
        Ok(None)
    });
}

pub fn multi_window_view_middle_click() {
    with_app("cerrar ventana por clic central", |app| {
        let target = match &mut app.preview {
            Some(view) => view.middle_click()?,
            None => None,
        };
        if let Some(hwnd) = target {
            nosih_shell::tracker::request_close_window(hwnd);
            if let Some(view) = &mut app.preview {
                let still_open = view.remove_window(&app.gfx, hwnd)?;
                if !still_open {
                    app.close_preview();
                }
            }
        }
        Ok(())
    });
}

pub fn multi_window_view_scroll(previous: bool) {
    with_app("más ventanas", |app| {
        if let Some(view) = &mut app.preview {
            view.scroll(&app.gfx, previous)?;
        }
        Ok(())
    });
}

pub fn multi_window_view_dismiss() {
    with_app("cerrar vistas", |app| {
        app.close_preview();
        app.render()
    });
}

pub fn group_view_tick() {
    with_app("grupo", |app| {
        if let Some(view) = &mut app.group_view
            && !view.tick()?
        {
            app.close_group_view();
        }
        // Capture from a background popup does not guarantee mouse-move
        // delivery once the pointer leaves it. Poll only its active press.
        if let Some(view) = &app.group_view
            && let Some((id, point)) = view.drag_started()?
        {
            app.start_group_drag(id, point)?;
        }
        Ok(())
    });
}

pub fn group_view_press() {
    with_app("grupo", |app| match &mut app.group_view {
        Some(view) => view.press(),
        None => Ok(()),
    });
}

pub fn group_view_moved() {
    with_app("arrastre desde un grupo", App::group_view_moved);
}

pub fn group_view_release() {
    with_app_then("abrir desde un grupo", App::group_view_released);
}

pub fn group_view_menu() {
    with_app_then("menú de un grupo", App::group_view_menu);
}

pub fn group_view_press_lost() {
    with_app("grupo", |app| {
        if let Some(view) = &mut app.group_view {
            view.cancel_press();
        }
        Ok(())
    });
}

pub fn menu_bar_check_pointer() {
    with_app("barra superior", |app| {
        if app.menu_open {
            Ok(())
        } else {
            match &mut app.menubar {
                Some(bar) => bar.check_pointer(&app.gfx),
                None => Ok(()),
            }
        }
    });
}

pub fn menu_bar_finish_hide(completed_epoch: Option<usize>) {
    with_app("ocultar barra superior", |app| match &mut app.menubar {
        Some(bar) => bar.finish_hide(&app.gfx, completed_epoch),
        None => Ok(()),
    });
}

pub fn menu_bar_pointer_moved(x: f32, y: f32) {
    with_app("barra superior", |app| match &mut app.menubar {
        Some(bar) => bar.pointer_moved(&app.gfx, x, y),
        None => Ok(()),
    });
}

pub fn menu_bar_dismiss_island() {
    with_app("barra superior", |app| match &mut app.menubar {
        Some(bar) => bar.dismiss_island(&app.gfx),
        None => Ok(()),
    });
}

/// Redraws the menu bar from what it reads on each draw (input language,
/// clock), without asking the network or audio again.
pub fn menu_bar_refresh() {
    with_app("barra superior", |app| {
        if let Some(bar) = &mut app.menubar {
            bar.refresh(&app.gfx)?;
        }
        app.take_bar_cue();
        Ok(())
    });
}

pub fn menu_bar_registry_changed() {
    with_app("barra superior", |app| match &mut app.menubar {
        Some(bar) => bar.registry_changed(&app.gfx),
        None => Ok(()),
    });
}

pub fn menu_bar_tick() {
    with_app("barra superior", |app| {
        if let Some(bar) = &mut app.menubar {
            bar.tick(&app.gfx)?;
        }
        app.take_bar_cue();
        Ok(())
    });
}

pub fn drag_enter(files: Vec<String>, x: i32, y: i32) -> DROPEFFECT {
    with_app_value("arrastre de archivos", |app| {
        app.on_drag_enter(files, POINT { x, y })
    })
}

pub fn drag_over(x: i32, y: i32) -> DROPEFFECT {
    with_app_value("arrastre de archivos", |app| {
        app.on_drag_over(POINT { x, y })
    })
}

pub fn drag_leave() {
    with_app("arrastre de archivos", App::on_drag_leave);
}

pub fn drop_files(x: i32, y: i32) -> DROPEFFECT {
    with_app_value("soltar archivos", |app| app.on_drop(POINT { x, y }))
}

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        BACKDROP_REFRESH => {
            BACKDROP_PENDING.store(false, Ordering::Relaxed);
            with_app("cristal del fondo de pantalla", App::refresh_backdrops);
        }
        HEALTH_CHECK => with_app("estado del dispositivo gráfico", |app| {
            app.gfx.check_device()
        }),
        WM_MOUSEACTIVATE => {
            let mouse = ((l.0 >> 16) & 0xFFFF) as u32;
            let activate = matches!(mouse, WM_LBUTTONDOWN | WM_MBUTTONDOWN)
                && with_app_value("activación por clic", |app| {
                    Ok(hwnd == app.input && app.activate_for_pointer())
                });
            // This is Windows' actual pointer activation negotiation. Preserve
            // the button message so the same click also opens the chosen app.
            return LRESULT(if activate { MA_ACTIVATE } else { MA_NOACTIVATE } as isize);
        }
        ASSETS_READY => {
            // SAFETY: Coalesce background results on this UI window.
            unsafe { SetTimer(Some(hwnd), ASSETS_TIMER, ASSETS_DELAY_MS, None) };
        }
        WM_TIMER if w.0 == ASSETS_TIMER => {
            // SAFETY: This UI thread owns the one-shot timer.
            unsafe {
                let _ = KillTimer(Some(hwnd), ASSETS_TIMER);
            }
            with_app("iconos del shell", App::assets_ready);
        }
        WM_MOUSEMOVE => with_app("movimiento del ratón", App::on_mouse_move),
        WM_MOUSELEAVE => with_app("salida del ratón", App::on_tracked_mouse_leave),
        WM_LBUTTONDOWN => with_app("pulsación", App::on_press),
        WM_LBUTTONUP => with_app_then("clic", App::on_release),
        WM_MBUTTONUP => with_app_then("clic central", App::on_middle_click),
        WM_CAPTURECHANGED => {
            // SAFETY: Posting a message to our own window.
            unsafe {
                let _ = PostMessageW(Some(hwnd), DRAG_CANCELLED, WPARAM(0), LPARAM(0));
            }
        }
        DRAG_CANCELLED => {
            // ReleaseCapture posts this too. A newer press may already
            // own capture by the time the deferred cancellation runs.
            if unsafe { GetCapture() } != hwnd {
                with_app("arrastre", App::cancel_drag);
            }
        }
        WM_TIMER if w.0 == DRAG_TIMER => with_app_then("arrastre", App::poll_drag),
        DROP_RUN => with_app_then("soltar archivos", |app| Ok(app.pending_drop.take())),
        WM_RBUTTONUP => with_app_then("menú", App::on_right_click),
        BIN_STATE => with_app("papelera", |app| app.set_bin_full(w.0 != 0)),
        TASKBAR_PREFS_CHANGED => {
            with_app("preferencias de badges", App::taskbar_preferences_changed)
        }
        BIN_CHANGED => {
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe { SetTimer(Some(hwnd), BIN_TIMER, BIN_DELAY_MS, None) };
        }
        WM_SETTINGCHANGE if w.0 == SPI_SETWORKAREA => {}
        WM_SETTINGCHANGE if w.0 == SPI_SETDESKWALLPAPER.0 as usize => {
            // FindWindow can return either dock HWND. Both must accept Settings'
            // notification; broadcast duplicates share one queued brush update.
            queue_backdrop_refresh(main_window());
        }
        WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
            queue_backdrop_refresh(main_window());
            if hwnd == main_window() {
                // Rebind the host sample now without discarding geometry or
                // closing previews. A real light/glass change is still picked
                // up by the normal scene keys when Windows finishes settling.
                // SAFETY: This is our live main UI window.
                unsafe {
                    SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_DELAY_MS, None);
                }
            }
        }
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            if matches!(msg, WM_DISPLAYCHANGE | WM_DPICHANGED) {
                with_app("pantalla de la mascota", |app| {
                    app.stop_mascot();
                    Ok(())
                });
            }
            // SAFETY: `hwnd` is our window; scheduling the same timer resets it.
            unsafe {
                SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_DELAY_MS, None);
                if msg == WM_DISPLAYCHANGE && hwnd == main_window() {
                    // Let Explorer settle and create any new secondary taskbars.
                    SetTimer(Some(hwnd), TASKBAR_TIMER, TASKBAR_DELAY_MS, None);
                }
            }
        }
        // On battery or charging, or energy saver turned on or off: glass may
        // have to become solid, or the other way round.
        WM_POWERBROADCAST if w.0 == PBT_APMPOWERSTATUSCHANGE || w.0 == PBT_POWERSETTINGCHANGE => {
            if w.0 == PBT_POWERSETTINGCHANGE {
                // SAFETY: Windows owns the setting payload for this message.
                unsafe {
                    power::power_setting_changed(l);
                }
            }
            // SAFETY: `hwnd` is our window; scheduling the same timer resets it.
            unsafe { SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_DELAY_MS, None) };
        }
        WM_TIMER if w.0 == BIN_TIMER => {
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe {
                let _ = KillTimer(Some(hwnd), w.0);
            }
            query_bin_in_background();
        }
        PINS_CHANGED => {
            with_app("apps fijadas", |app| {
                if let Some(watch) = &mut app.pin_watch {
                    watch.arm();
                }
                Ok(())
            });
            // SAFETY: `hwnd` is our window; scheduling the same timer resets it.
            unsafe { SetTimer(Some(hwnd), PINS_TIMER, PINS_DELAY_MS, None) };
        }
        WM_TIMER if w.0 == PINS_TIMER => {
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe {
                let _ = KillTimer(Some(hwnd), w.0);
            }
            with_app("apps fijadas", App::sync_taskbar_pins);
        }
        WM_TIMER if w.0 == UPDATE_TIMER => {
            with_app("actualización", |app| {
                app.update_now();
                Ok(())
            });
        }
        WM_TIMER if w.0 == SWITCHER_TIMER => {
            with_app("Alt+Tab", App::check_task_switcher);
        }
        WM_TIMER if w.0 == FULLSCREEN_TIMER => {
            with_app("pantalla completa", App::update_fullscreen);
        }
        WM_TIMER if w.0 == SPRING_TIMER => {
            with_app_then("arrastre sobre una app", App::spring_open);
        }
        WM_TIMER if w.0 == MASCOT_TIMER => with_app("mascota", App::mascot_tick),
        WM_TIMER if w.0 == WELCOME_TIMER => with_app("bienvenida", App::welcome),
        WM_TIMER if w.0 == BREAK_TIMER => with_app("descanso", App::break_tick),
        // Unlocked: maybe the first time today (see `App::welcome`).
        WM_WTSSESSION_CHANGE if w.0 == WTS_SESSION_UNLOCK as usize => {
            with_app("desbloqueo", |app| {
                app.arm_welcome();
                Ok(())
            });
        }
        edge_watch::CHANGED => with_app("borde de pantalla", App::edge_changed),
        WM_TIMER if w.0 == EDGE_TIMER => with_app("mostrar al llegar al borde", App::edge_rested),
        WM_TIMER if w.0 == PREVIEW_TIMER => with_app("selector de ventanas", App::preview_tick),
        WM_TIMER if w.0 == HIDE_TIMER => {
            // SAFETY: Same as above.
            unsafe {
                let _ = KillTimer(Some(hwnd), w.0);
            }
            with_app("ocultación", App::hide_if_unused);
        }
        WM_TIMER if w.0 == TRIM_TIMER => {
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe {
                let _ = KillTimer(Some(hwnd), w.0);
            }
            with_app("recorte de memoria", |app| {
                app.trim_if_idle();
                Ok(())
            });
        }
        WM_TIMER if [REFRESH_TIMER, SYNC_TIMER, TASKBAR_TIMER].contains(&w.0) => {
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe {
                let _ = KillTimer(Some(hwnd), w.0);
            }
            match w.0 {
                REFRESH_TIMER => with_app("recomposición del dock", |app| {
                    app.apply_edge_settings();
                    app.refresh()?;
                    app.update_fullscreen()?;
                    app.power_moment();
                    Ok(())
                }),
                SYNC_TIMER => with_app("lectura de ventanas", App::sync),
                _ => with_app("ocultar la barra de Windows", |app| {
                    app.request_taskbar(true)
                }),
            }
        }
        APPBAR_CALLBACK => with_app("aviso de AppBar", |app| app.on_appbar_notice(w.0)),
        // Once the taskbar is set to auto-hide, the dock strip moves to the
        // screen edge, so reserve and reposition it again.
        TASKBAR_TAKEN => with_app("reserva de espacio", |app| {
            app.taskbar_taken(w.0 != 0, l.0 != 0)
        }),
        WM_POWERBROADCAST if w.0 == PBT_APMRESUMEAUTOMATIC => {
            with_app("reanudar reserva de espacio", |app| {
                app.taskbar_attempts = 0;
                // Asleep was a pause; awake may be the first time today.
                app.moments.rested();
                app.arm_welcome();
                Ok(())
            });
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe { SetTimer(Some(hwnd), TASKBAR_TIMER, TASKBAR_DELAY_MS, None) };
            // Timers do not count the time asleep: aim it again by the clock.
            with_app("actualización", |app| {
                app.schedule_update();
                Ok(())
            });
        }
        _ if CONFIG_CHANGED.get() == Some(&msg) => with_app("ajustes", App::reload_config),
        crate::overlays::OVERLAYS_CHANGED => {
            with_app("insignias de apps", |app| {
                app.sync_badge_visuals()?;
                app.render()
            });
        }
        _ if TASKBAR_CREATED.get() == Some(&msg) => {
            // The new taskbar points its overlay calls at itself again.
            crate::overlays::reclaim();
            with_app("Explorer reiniciado", |app| {
                app.taskbar_attempts = 0;
                Ok(())
            });
            // SAFETY: `hwnd` is this live window on its owning UI thread.
            unsafe { SetTimer(Some(hwnd), TASKBAR_TIMER, TASKBAR_DELAY_MS, None) };
        }
        // A folder's submenu in the shortcuts folder's menu is about to open.
        WM_INITMENUPOPUP if folder_menu::init_popup(HMENU(w.0 as *mut _)) => {}
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
        }
        _ if SHELL_HOOK.get() == Some(&msg) => match w.0 {
            // Changing the foreground window does not change the app list; it
            // can only enter or exit full-screen mode.
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => {
                with_app("ventana activa", App::on_foreground_changed);
            }
            // Terminals and browsers retitle their windows many times a minute.
            // The dock shows no titles, so only a window it does not list yet
            // (an app that declares its identity late) needs a new look.
            HSHELL_REDRAW if is_tracked(l.0) => {}
            // The input language indicator of the menu bar.
            HSHELL_LANGUAGE => menu_bar_refresh(),
            code => {
                if code == HSHELL_FLASH {
                    with_app("aviso de atención", |app| app.on_attention(l.0));
                } else if code == HSHELL_WINDOWDESTROYED {
                    with_app("ventana cerrada", |app| app.forget_attention(l.0));
                }
                // SAFETY: `hwnd` is this live window on its owning UI thread.
                unsafe { SetTimer(Some(hwnd), SYNC_TIMER, SYNC_DELAY_MS, None) };
            }
        },
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW, WS_EX_TOOLWINDOW,
            WS_POPUP,
        },
        core::w,
    };

    struct Window(HWND);
    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: Only this test's hidden window is destroyed.
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    #[test]
    #[ignore = "Native event queue regression; hidden test window, no desktop changes"]
    fn wallpaper_notifications_coalesce_and_can_be_posted_again() -> Result<()> {
        // SAFETY: A hidden STATIC window owned solely by this test thread.
        let window = Window(unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Nosih backdrop regression"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        });
        queue_backdrop_refresh(HWND::default());
        assert!(!BACKDROP_PENDING.load(Ordering::Relaxed));
        queue_backdrop_refresh(HWND(std::ptr::dangling_mut())); // Failed posts must not latch.
        assert!(!BACKDROP_PENDING.load(Ordering::Relaxed));
        for _ in 0..8 {
            queue_backdrop_refresh(window.0);
        }
        let mut message = MSG::default();
        // SAFETY: Only payload-free notifications on the test-owned window.
        unsafe {
            assert!(
                PeekMessageW(
                    &mut message,
                    Some(window.0),
                    BACKDROP_REFRESH,
                    BACKDROP_REFRESH,
                    PM_REMOVE
                )
                .as_bool()
            );
            assert!(
                !PeekMessageW(
                    &mut message,
                    Some(window.0),
                    BACKDROP_REFRESH,
                    BACKDROP_REFRESH,
                    PM_REMOVE
                )
                .as_bool()
            );
            // No App lives on this test thread; this acknowledges the event
            // without touching the real desktop or constructing its UI.
            wndproc(window.0, BACKDROP_REFRESH, WPARAM(0), LPARAM(0));
            queue_backdrop_refresh(window.0);
            assert!(
                PeekMessageW(
                    &mut message,
                    Some(window.0),
                    BACKDROP_REFRESH,
                    BACKDROP_REFRESH,
                    PM_REMOVE
                )
                .as_bool()
            );
            wndproc(window.0, BACKDROP_REFRESH, WPARAM(0), LPARAM(0));
        }
        assert!(!BACKDROP_PENDING.load(Ordering::Relaxed));
        Ok(())
    }
}
