//! What a window reports about its own rectangle at the moment it starts to
//! minimize (EVENT_SYSTEM_MINIMIZESTART), when it was maximized.

use std::{thread::sleep, time::Duration};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                EVENT_SYSTEM_MINIMIZESTART, GetWindowPlacement, GetWindowRect, IsIconic, MSG,
                PM_REMOVE, PeekMessageW, RegisterClassW, SW_MAXIMIZE, SW_MINIMIZE, ShowWindow,
                WINDOWPLACEMENT, WINEVENT_OUTOFCONTEXT, WNDCLASSW, WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::w,
};

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Default handling.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    // SAFETY: Read-only queries on a live window.
    unsafe {
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let mut frame = RECT::default();
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        );
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        let _ = GetWindowPlacement(hwnd, &mut placement);
        println!(
            "computed (whole, visible): {:?}",
            nosih_shell::tracker::rect_before_minimize(hwnd.0 as isize, None)
        );
        println!(
            "at MINIMIZESTART: iconic={} window={:?} frame={:?} flags={:#x} showCmd={} normal={:?}",
            IsIconic(hwnd).as_bool(),
            rect,
            frame,
            placement.flags.0,
            placement.showCmd,
            placement.rcNormalPosition
        );
    }
}

fn pump(ms: u64) {
    let mut msg = MSG::default();
    for _ in 0..ms / 5 {
        // SAFETY: Standard pump.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        sleep(Duration::from_millis(5));
    }
}

fn main() {
    // SAFETY: Our own window and hook.
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("NosihSpikeMinRect"),
            ..Default::default()
        });
        let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            w!("NosihSpikeMinRect"),
            w!("min rect"),
            WS_OVERLAPPEDWINDOW,
            300,
            200,
            700,
            450,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _hook = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZESTART,
            None,
            Some(on_event),
            std::process::id(),
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        pump(600);
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let mut frame = RECT::default();
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        );
        println!("maximized on screen: window={rect:?} visible={frame:?}");
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        pump(600);
        let _ = DestroyWindow(hwnd);
    }
}
