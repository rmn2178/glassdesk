//! Observes screen edges on a dedicated timer thread while auto-hide is active.

use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::Result;
use nosih_core::frame::DockEdge;
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        GetCursorPos, GetMessageW, KillTimer, MSG, PM_NOREMOVE, PeekMessageW, PostMessageW,
        PostThreadMessageW, SetTimer, WM_APP, WM_QUIT, WM_TIMER,
    },
};

pub const CHANGED: u32 = WM_APP + 26;
const DWELL: Duration = Duration::from_millis(250);
const MENU_DWELL: Duration = Duration::from_millis(150);
const EDGE_DEPTH: i32 = 2;
const SAMPLE_MS: u32 = 80;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    MenuBar,
    Dock(RECT),
}

impl Target {
    fn delay(self) -> Duration {
        match self {
            Self::MenuBar => MENU_DWELL,
            Self::Dock(_) => DWELL,
        }
    }
}

pub enum Reveal {
    Idle,
    Wait(Duration),
    Ready(Target),
}

#[derive(Default)]
struct Edges {
    window: isize,
    top: Option<RECT>,
    /// The displays a hidden dock can be revealed on, and the edge it sits on.
    screens: Vec<RECT>,
    dock_edge: DockEdge,
    pointer_screen: Option<RECT>,
    target: Option<Target>,
    entered: Option<Instant>,
}

static EDGES: Mutex<Edges> = Mutex::new(Edges {
    window: 0,
    top: None,
    screens: Vec::new(),
    dock_edge: DockEdge::Bottom,
    pointer_screen: None,
    target: None,
    entered: None,
});

impl Edges {
    fn at(&self, point: POINT, screen: Option<RECT>) -> Option<Target> {
        if self.top.is_some_and(|top| {
            (top.left..top.right).contains(&point.x)
                && (top.top..top.top + EDGE_DEPTH).contains(&point.y)
        }) {
            return Some(Target::MenuBar);
        }
        // The menu bar, checked first, keeps the top corners.
        screen
            .filter(|screen| match self.dock_edge {
                DockEdge::Bottom => point.y >= screen.bottom - EDGE_DEPTH,
                DockEdge::Left => point.x < screen.left + EDGE_DEPTH,
                DockEdge::Right => point.x >= screen.right - EDGE_DEPTH,
            })
            .map(Target::Dock)
    }

    fn observe(&mut self, point: POINT, now: Instant) -> bool {
        let screen = self.screens.iter().copied().find(|screen| {
            (screen.left..screen.right).contains(&point.x)
                && (screen.top..screen.bottom).contains(&point.y)
        });
        let changed_screen = self.pointer_screen != screen;
        self.pointer_screen = screen;
        let target = self.at(point, screen);
        if target == self.target {
            return changed_screen;
        }
        self.target = target;
        self.entered = target.map(|_| now);
        true
    }

    fn set_top(&mut self, top: Option<RECT>) {
        if top != self.top {
            self.top = top;
            if self.target == Some(Target::MenuBar) {
                self.target = None;
                self.entered = None;
            }
        }
    }

    fn set_screens(&mut self, screens: Vec<RECT>, edge: DockEdge) {
        if let Some(Target::Dock(screen)) = self.target
            && (!screens.contains(&screen) || edge != self.dock_edge)
        {
            self.target = None;
            self.entered = None;
        }
        self.screens = screens;
        self.dock_edge = edge;
    }

    fn dwell(&mut self, point: POINT, now: Instant) -> Reveal {
        self.observe(point, now);
        let (Some(entered), Some(target)) = (self.entered, self.target) else {
            return Reveal::Idle;
        };
        let elapsed = now.saturating_duration_since(entered);
        let delay = target.delay();
        if elapsed < delay {
            return Reveal::Wait(delay - elapsed);
        }
        self.entered = None;
        Reveal::Ready(target)
    }
}

fn lock() -> MutexGuard<'static, Edges> {
    EDGES.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn set_menu_bar(top: Option<RECT>) {
    lock().set_top(top);
}

/// The displays a hidden dock can come out on, from `edge` of each.
pub fn set_dock_monitors(screens: Vec<RECT>, edge: DockEdge) {
    lock().set_screens(screens, edge);
}

pub fn poll(point: POINT) -> Reveal {
    lock().dwell(point, Instant::now())
}

/// Dwell is owned by the UI thread while observations arrive from the watcher.
pub fn pending() -> (Option<Target>, Option<Duration>) {
    let edges = lock();
    (
        edges.target,
        edges
            .entered
            .zip(edges.target)
            .map(|(entered, target)| target.delay().saturating_sub(entered.elapsed())),
    )
}

pub fn retry_on_move(target: Target) {
    let mut edges = lock();
    if edges.target == Some(target) {
        edges.target = None;
        edges.entered = None;
    }
}

pub struct EdgeWatch {
    thread: u32,
    join: Option<JoinHandle<()>>,
}

impl EdgeWatch {
    pub fn start(window: HWND) -> Result<Self> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let join = thread::Builder::new()
            .name("nosih-edge".into())
            .spawn(move || {
                // SAFETY: This thread owns the message queue and timer.
                unsafe {
                    let mut msg = MSG::default();
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let timer = SetTimer(None, 0, SAMPLE_MS, None);
                    if timer == 0 {
                        let _ = sender.send(Err(windows::core::Error::from_thread()));
                        return;
                    }
                    let _ = sender.send(Ok(GetCurrentThreadId()));
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        if msg.message == WM_TIMER {
                            sample_cursor();
                        }
                    }
                    let _ = KillTimer(None, timer);
                }
            })?;
        match receiver.recv() {
            Ok(Ok(thread)) => {
                lock().window = window.0 as isize;
                Ok(Self {
                    thread,
                    join: Some(join),
                })
            }
            result => {
                let _ = join.join();
                Err(anyhow::anyhow!("no se pudo observar el borde: {result:?}"))
            }
        }
    }
}

impl Drop for EdgeWatch {
    fn drop(&mut self) {
        {
            let mut edges = lock();
            edges.window = 0;
            edges.pointer_screen = None;
            edges.target = None;
            edges.entered = None;
        }
        // SAFETY: Posting to the queue created before the thread reported ready.
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn sample_cursor() {
    let mut point = POINT::default();
    // SAFETY: The output is live and this thread only reads cursor position.
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return;
    }
    let window = EDGES.try_lock().ok().and_then(|mut edges| {
        (edges.window != 0 && edges.observe(point, Instant::now())).then_some(edges.window)
    });
    if let Some(window) = window {
        // SAFETY: Post a scalar notification without waiting for the UI.
        unsafe {
            let _ = PostMessageW(Some(HWND(window as *mut _)), CHANGED, WPARAM(0), LPARAM(0));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a Windows desktop and a stationary pointer"]
    fn watcher_detects_a_pointer_already_at_the_edge() -> Result<()> {
        use windows::{
            Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, PM_REMOVE, WS_POPUP},
            core::w,
        };
        // SAFETY: A test-owned hidden receiving window. The test reads the
        // cursor and pumps only its own messages without injecting input.
        unsafe {
            let window = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Nosih edge test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?;
            let result = (|| -> Result<()> {
                let mut point = POINT::default();
                GetCursorPos(&mut point)?;
                set_menu_bar(Some(RECT {
                    left: point.x - 200,
                    right: point.x + 200,
                    top: point.y,
                    bottom: point.y + 30,
                }));
                let watch = EdgeWatch::start(window)?;
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut msg = MSG::default();
                let mut received = false;
                while Instant::now() < deadline {
                    if PeekMessageW(&mut msg, Some(window), CHANGED, CHANGED, PM_REMOVE).as_bool() {
                        received = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                assert!(
                    received,
                    "edge notification must arrive without a new mouse event"
                );
                drop(watch);
                set_menu_bar(None);
                Ok(())
            })();
            let _ = DestroyWindow(window);
            result
        }
    }

    #[test]
    fn a_side_dock_comes_out_of_its_side_and_the_menu_bar_keeps_the_corner() {
        let screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mut edges = Edges::default();
        edges.set_top(Some(RECT {
            bottom: 35,
            ..screen
        }));
        edges.set_screens(vec![screen], DockEdge::Left);
        assert_eq!(
            edges.at(POINT { x: 0, y: 500 }, Some(screen)),
            Some(Target::Dock(screen))
        );
        assert_eq!(edges.at(POINT { x: 900, y: 1079 }, Some(screen)), None);
        assert_eq!(
            edges.at(POINT { x: 0, y: 0 }, Some(screen)),
            Some(Target::MenuBar)
        );
        edges.set_screens(vec![screen], DockEdge::Right);
        assert_eq!(
            edges.at(POINT { x: 1919, y: 500 }, Some(screen)),
            Some(Target::Dock(screen))
        );
        assert_eq!(edges.at(POINT { x: 0, y: 500 }, Some(screen)), None);
    }

    #[test]
    fn menu_bar_reveals_sooner_without_changing_dock_switching_delay() {
        let start = Instant::now();
        let screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let top = POINT { x: 700, y: 0 };
        let bottom = POINT { x: 700, y: 1079 };
        let mut edges = Edges::default();
        edges.set_top(Some(screen));
        edges.set_screens(vec![screen], DockEdge::Bottom);
        edges.observe(top, start);
        assert!(matches!(
            edges.dwell(top, start + MENU_DWELL),
            Reveal::Ready(Target::MenuBar)
        ));
        edges.observe(bottom, start);
        assert!(matches!(
            edges.dwell(bottom, start + MENU_DWELL),
            Reveal::Wait(_)
        ));
        assert!(matches!(
            edges.dwell(bottom, start + DWELL),
            Reveal::Ready(Target::Dock(_))
        ));
    }

    #[test]
    fn an_unchanged_bar_layout_does_not_cancel_hover_dwell() {
        let mut edges = Edges::default();
        let screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let point = POINT { x: 700, y: 1 };
        let start = Instant::now();
        edges.set_top(Some(screen));
        assert!(edges.observe(point, start));
        edges.set_top(Some(screen));
        assert!(matches!(
            edges.dwell(point, start + DWELL),
            Reveal::Ready(Target::MenuBar)
        ));
        assert!(matches!(edges.dwell(point, start + DWELL), Reveal::Idle));
    }

    #[test]
    fn dwell_follows_the_bottom_of_the_display_with_the_pointer() {
        let primary = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let secondary = RECT {
            left: -1280,
            top: -200,
            right: 0,
            bottom: 824,
        };
        let mut edges = Edges::default();
        edges.set_screens(vec![primary, secondary], DockEdge::Bottom);
        let start = Instant::now();
        let left = POINT { x: -700, y: 823 };
        let right = POINT { x: 700, y: 1079 };
        assert!(edges.observe(right, start));
        assert!(edges.observe(left, start + Duration::from_millis(100)));
        assert!(matches!(edges.dwell(left, start + DWELL), Reveal::Wait(_)));
        assert!(
            matches!(edges.dwell(left, start + DWELL * 2), Reveal::Ready(Target::Dock(screen)) if screen == secondary)
        );
        edges.set_screens(vec![primary], DockEdge::Bottom);
        assert!(matches!(edges.dwell(left, start + DWELL * 3), Reveal::Idle));
    }

    #[test]
    fn monitor_entry_notifies_before_the_pointer_reaches_the_edge() {
        let primary = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let secondary = RECT {
            left: 1920,
            right: 3840,
            ..primary
        };
        let mut edges = Edges::default();
        edges.set_screens(vec![primary, secondary], DockEdge::Bottom);
        let now = Instant::now();
        assert!(edges.observe(POINT { x: 700, y: 500 }, now));
        assert!(edges.observe(POINT { x: 2500, y: 500 }, now));
        assert_eq!(edges.target, None);
        assert!(!edges.observe(POINT { x: 2600, y: 500 }, now));
    }

    #[test]
    fn pending_reveal_cancels_when_the_pointer_crosses_the_edge() {
        let mut edges = Edges::default();
        edges.set_screens(
            vec![RECT {
                left: 0,
                top: -900,
                right: 1600,
                bottom: 0,
            }],
            DockEdge::Bottom,
        );
        let start = Instant::now();
        assert!(edges.observe(POINT { x: 800, y: -1 }, start));
        assert!(matches!(
            edges.dwell(POINT { x: 800, y: 0 }, start + DWELL),
            Reveal::Idle
        ));
    }
}
