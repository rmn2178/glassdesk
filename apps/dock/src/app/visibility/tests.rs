//! Regression harness: hidden test-owned HWNDs, no taskbar takeover or settings writes.
use super::super::{ASSETS_READY, App, HOVER_OMEGA, Press, PreviewOrder};
use crate::{assets::Visuals, gfx::Gfx, thumbnails::Thumbnails};
use anyhow::Result;
use nosih_config::Config;
use nosih_core::{attention::Attention, frame::DockEdge, roster::Roster, spring::Spring};
use nosih_shell::{power, tracker::WindowTracker};
use std::{collections::HashMap, time::Instant};
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        System::WinRT::{
            Composition::ICompositorDesktopInterop, CreateDispatcherQueueController, DQTAT_COM_STA,
            DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_TOOLWINDOW, WS_POPUP,
        },
    },
    core::{Interface, w},
};

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
struct Fixture {
    app: App,
    _windows: [Window; 2],
}
impl Fixture {
    fn new() -> Result<Self> {
        let create = || unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Nosih hiding regression"),
                WS_POPUP,
                -32000,
                -32000,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .map(Window)
        };
        let windows = [create()?, create()?];
        let visual = windows[0].0;
        let input = windows[1].0;
        let gfx = Gfx::new()?;
        let target = unsafe {
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(visual, true)?
        };
        let config = Config::default();
        let app = App {
            visual,
            input,
            gfx,
            target,
            appbar: None,
            taskbar_worker: None,
            taskbar_retry: false,
            taskbar_attempts: 0,
            fullscreen: false,
            smart_hidden: false,
            roster: Roster::default(),
            tracker: WindowTracker::default(),
            windows: Vec::new(),
            preview_order: PreviewOrder::default(),
            last_rects: HashMap::new(),
            entries: Vec::new(),
            icons: Visuals::new(visual, ASSETS_READY)?,
            downloads: None,
            bin_full: false,
            _bin_watch: None,
            _saver_watch: power::SaverWatch::new(input.0 as isize),
            pin_watch: None,
            badge_watch: None,
            attention: Attention::default(),
            attention_enabled: false,
            assets: HashMap::new(),
            assets_for: (0.0, false, 0, DockEdge::Bottom),
            placeholder: None,
            remove_label: None,
            config,
            slide: Spring::at(0.0, 24.0),
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
            cursor_spring: nosih_core::spring::ElasticSpring::at(
                0.0,
                crate::app::CURSOR_OMEGA,
                crate::app::CURSOR_DAMPING,
            ),
            amount: nosih_core::spring::ElasticSpring::at(
                0.0,
                HOVER_OMEGA,
                crate::app::HOVER_DAMPING,
            ),
            press_scale: nosih_core::spring::ElasticSpring::at(
                1.0,
                crate::app::PRESS_OMEGA,
                crate::app::PRESS_DAMPING,
            ),
            press_index: None,
            last_frame: Instant::now(),
        };
        Ok(Self {
            app,
            _windows: windows,
        })
    }
}
fn queue() -> windows::core::Result<windows::System::DispatcherQueueController> {
    unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })
    }
}

#[test]
#[ignore = "Native hiding regression; only hidden test-owned windows, no user settings writes"]
fn native_hiding_reconciles_all_modes_when_the_desktop_returns() -> Result<()> {
    let _queue = queue()?;
    let mut fixture = Fixture::new()?;
    let app = &mut fixture.app;
    for auto in [false, true] {
        for smart in [false, true] {
            for overlap in [false, true] {
                app.config.auto_hide = auto;
                app.config.smart_hide = smart;
                app.smart_hidden = !auto && smart && overlap;
                let hidden = auto || (smart && overlap);
                app.slide = Spring::at(if hidden { 0.0 } else { 1.0 }, 24.0);
                app.hide_if_unused()?;
                assert_eq!(
                    app.slide.target,
                    if hidden { 1.0 } else { 0.0 },
                    "auto={auto}, smart={smart}, overlap={overlap}"
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "Native hiding regression; only hidden test-owned windows, no user settings writes"]
fn native_hiding_does_not_interrupt_a_pending_press_or_open_menu() -> Result<()> {
    let _queue = queue()?;
    let mut fixture = Fixture::new()?;
    let app = &mut fixture.app;
    for auto in [false, true] {
        app.config.auto_hide = auto;
        app.config.smart_hide = true;
        app.smart_hidden = !auto;
        app.slide = Spring::at(0.0, 24.0);
        app.press = Some(Press {
            id: "test-app".into(),
            at: POINT::default(),
            draggable: true,
        });
        app.hide_if_unused()?;
        assert_eq!(
            app.slide.target, 0.0,
            "a pending icon press must keep the dock visible"
        );
        app.press = None;
        app.menu_open = true;
        app.hide_if_unused()?;
        assert_eq!(
            app.slide.target, 0.0,
            "an open menu must keep the dock visible"
        );
        app.menu_open = false;
        app.hide_if_unused()?;
        assert_eq!(
            app.slide.target, 1.0,
            "hiding resumes once the interaction ends"
        );
        // A hide timeout can fire during the press. Releasing or cancelling
        // must rearm that same one-shot delay instead of leaving the dock up.
        app.slide = Spring::at(0.0, 24.0);
        app.press = Some(Press {
            id: "test-app".into(),
            at: POINT::default(),
            draggable: false,
        });
        assert!(app.on_release()?.is_none());
        assert!(unsafe {
            windows::Win32::UI::WindowsAndMessaging::KillTimer(
                Some(app.input),
                super::super::HIDE_TIMER,
            )
            .is_ok()
        });
        app.press = Some(Press {
            id: "test-app".into(),
            at: POINT::default(),
            draggable: false,
        });
        app.cancel_drag()?;
        assert!(unsafe {
            windows::Win32::UI::WindowsAndMessaging::KillTimer(
                Some(app.input),
                super::super::HIDE_TIMER,
            )
            .is_ok()
        });
    }
    Ok(())
}

#[test]
#[ignore = "Native hiding regression; only hidden test-owned windows, no user settings writes"]
fn native_hiding_clears_overlap_when_automatic_hiding_takes_precedence() -> Result<()> {
    let _queue = queue()?;
    let mut fixture = Fixture::new()?;
    let app = &mut fixture.app;
    app.config.auto_hide = true;
    app.config.smart_hide = true;
    app.smart_hidden = true; // Previous smart-only mode.
    app.slide = Spring::at(1.0, 24.0);
    app.update_smart_hiding()?;
    assert!(
        !app.smart_hidden,
        "automatic mode must not retain intelligent overlap state"
    );
    assert_eq!(
        app.slide.target, 1.0,
        "the desktop does not override automatic hiding"
    );
    app.config.auto_hide = false;
    app.hide_if_unused()?;
    assert_eq!(
        app.slide.target, 0.0,
        "smart-only mode shows an unobstructed desktop"
    );
    Ok(())
}
