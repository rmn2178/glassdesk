//! Counts how often the menu bar's status watches fire while nothing changes.
//! Usage: spike_status [seconds]

use std::{
    env,
    time::{Duration, Instant},
};

use nosih_shell::status::{self, AudioWatch, NetworkWatch};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::WindowsAndMessaging::{
            CreateWindowExW, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW, WINDOW_EX_STYLE, WM_APP,
            WS_OVERLAPPED,
        },
    },
    core::w,
};

fn main() {
    let seconds: u64 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    // SAFETY: COM for this thread; a message-only window owned by this thread.
    let hwnd = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .unwrap_or_default()
    };
    let message = WM_APP + 21;
    let _network = NetworkWatch::new(hwnd.0 as isize, message);
    let mut audio = AudioWatch::new(hwnd.0 as isize, message);
    println!(
        "start: network={:?} volume={:?} buttons={:?}",
        status::network(),
        audio.as_mut().and_then(AudioWatch::volume),
        status::taskbar_buttons()
    );

    let mut events = 0;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        let mut msg = MSG::default();
        // SAFETY: Reads this thread's queue into `msg`.
        while unsafe { PeekMessageW(&mut msg, Some(HWND(hwnd.0)), 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == message {
                events += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("status events in {seconds} s: {events}");
}
