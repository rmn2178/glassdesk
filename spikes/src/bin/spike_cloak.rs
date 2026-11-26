//! Counts window cloak and uncloak events (virtual desktop switches, Start,
//! some Store apps) for a while, to check what listening to them costs.
//! Usage: `spike_cloak [seconds]`.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::HWND,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED, MSG, OBJID_WINDOW,
            PM_REMOVE, PeekMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        },
    },
};

static EVENTS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if object == OBJID_WINDOW.0 {
        EVENTS.fetch_add(1, Ordering::Relaxed);
    }
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    // SAFETY: An out-of-context hook delivered through this thread's message loop.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CLOAKED,
            EVENT_OBJECT_UNCLOAKED,
            None,
            Some(on_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut msg = MSG::default();
    while Instant::now() < until {
        // SAFETY: Standard message pump on this thread.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: The hook installed above.
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
    println!(
        "cloak/uncloak events in {seconds} s: {}",
        EVENTS.load(Ordering::Relaxed)
    );
}
