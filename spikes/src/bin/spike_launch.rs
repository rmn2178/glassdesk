//! Checks the dock's launch, activate and minimize paths without a mouse.
//! Usage: spike_launch <exe path> <window title fragment>
//! Waits until the user has been idle for 30 s, because it moves the focus.

use std::{env, thread, time::Duration};

use nosih_shell::launcher;
use windows::Win32::{
    Foundation::HWND,
    System::SystemInformation::GetTickCount,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindowTextW, IsIconic, IsWindowVisible,
        },
    },
};
use windows::core::BOOL;

fn idle_ms() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` has its size set and lives for the call.
    unsafe {
        let _ = GetLastInputInfo(&mut info);
        GetTickCount().wrapping_sub(info.dwTime)
    }
}

fn find(fragment: &str) -> Option<HWND> {
    unsafe extern "system" fn collect(
        hwnd: HWND,
        list: windows::Win32::Foundation::LPARAM,
    ) -> BOOL {
        // SAFETY: `list` is the Vec below, alive for the synchronous enumeration.
        unsafe { (*(list.0 as *mut Vec<HWND>)).push(hwnd) };
        true.into()
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: The Vec outlives EnumWindows.
    unsafe {
        let _ = EnumWindows(
            Some(collect),
            windows::Win32::Foundation::LPARAM(&raw mut all as isize),
        );
    }
    let needle = fragment.to_lowercase();
    all.into_iter().find(|&hwnd| {
        let mut buffer = [0u16; 256];
        // SAFETY: Read-only queries.
        let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
        let title = String::from_utf16_lossy(&buffer[..len.max(0) as usize]).to_lowercase();
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        visible && title.contains(&needle)
    })
}

fn report(step: &str, hwnd: HWND) {
    // SAFETY: Read-only queries.
    let (front, iconic) = unsafe { (GetForegroundWindow() == hwnd, IsIconic(hwnd).as_bool()) };
    println!("{step}: foreground={front} minimized={iconic}");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let [exe, fragment] = args.as_slice() else {
        eprintln!("usage: spike_launch <exe> <title fragment>");
        return;
    };
    while idle_ms() < 30_000 {
        thread::sleep(Duration::from_secs(2));
    }

    if let Err(error) = launcher::open(exe) {
        println!("open failed: {error}");
        return;
    }
    let mut hwnd = None;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(200));
        hwnd = find(fragment);
        if hwnd.is_some() {
            break;
        }
    }
    let Some(hwnd) = hwnd else {
        println!("no window titled like {fragment:?} appeared");
        return;
    };
    thread::sleep(Duration::from_millis(800));
    report("after open", hwnd);

    let raw = hwnd.0 as isize;
    launcher::minimize(&[raw]);
    thread::sleep(Duration::from_millis(800));
    report("after minimize", hwnd);

    launcher::activate(&[raw]);
    thread::sleep(Duration::from_millis(800));
    report("after activate", hwnd);

    launcher::minimize(&[raw]);
    thread::sleep(Duration::from_millis(800));
    report("after second minimize", hwnd);
    launcher::activate(&[raw]);
    thread::sleep(Duration::from_millis(800));
    report("after second activate", hwnd);
}
