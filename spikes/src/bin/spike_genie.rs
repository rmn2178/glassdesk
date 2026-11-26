//! A plain magenta window that minimizes itself, to time the genie's take-off
//! and see what is on screen while it flies.
//!
//! Usage: `spike_genie [cycles] [probe_ms]`. After each minimize it waits
//! `probe_ms` and reports how much of the window's rectangle is still magenta
//! in the top and bottom rows of the screen copy.

use std::{thread::sleep, time::Duration};

use nosih_shell::screen;
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::CreateSolidBrush,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
            PeekMessageW, RegisterClassW, SW_MINIMIZE, SW_RESTORE, SW_SHOWNOACTIVATE, ShowWindow,
            WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Default handling of our own window's messages.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn pump(ms: u64) {
    let mut msg = MSG::default();
    for _ in 0..ms / 5 {
        // SAFETY: Standard message pump on the thread that owns the window.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        sleep(Duration::from_millis(5));
    }
}

/// Share of magenta pixels in row `y` of the rectangle.
fn magenta(rect: RECT, y: i32) -> f32 {
    let row = RECT {
        left: rect.left,
        top: y,
        right: rect.right,
        bottom: y + 1,
    };
    screen::capture(row).map_or(-1.0, |image| {
        let pixels = image.bgra.as_chunks::<4>().0;
        let hits = pixels
            .iter()
            .filter(|pixel| pixel[0] > 240 && pixel[1] < 15 && pixel[2] > 240)
            .count();
        hits as f32 / pixels.len().max(1) as f32
    })
}

/// Opens and closes `cycles` extra windows of this app, so the dock adds and
/// removes them, to look for leaks.
fn churn(first: HWND, cycles: u32) {
    pump(2500);
    for _ in 0..cycles {
        // SAFETY: Creates and destroys windows of the class registered in `main`.
        unsafe {
            let Ok(instance) = GetModuleHandleW(None) else {
                return;
            };
            let Ok(extra) = CreateWindowExW(
                Default::default(),
                w!("NosihSpikeGenie"),
                w!("Genie spike churn"),
                WS_OVERLAPPEDWINDOW,
                400,
                200,
                600,
                400,
                None,
                None,
                Some(instance.into()),
                None,
            ) else {
                return;
            };
            let _ = ShowWindow(extra, SW_SHOWNOACTIVATE);
            pump(600);
            let _ = DestroyWindow(extra);
            pump(600);
        }
    }
    // SAFETY: Our own window.
    unsafe {
        let _ = DestroyWindow(first);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cycles: u32 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let probe: u64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(250);
    let rect = RECT {
        left: 300,
        top: 150,
        right: 1300,
        bottom: 750,
    };
    // SAFETY: Standard class registration and window creation.
    let hwnd = unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("NosihSpikeGenie");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: class,
            hbrBackground: CreateSolidBrush(COLORREF(0x00FF_00FF)),
            ..Default::default()
        });
        let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            class,
            w!("Genie spike"),
            WS_OVERLAPPEDWINDOW,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        hwnd
    };
    if std::env::var_os("SPIKE_CHURN").is_some() {
        churn(hwnd, cycles);
        return;
    }
    pump(2500);
    // Rows well inside the client area, near its top and its bottom.
    let (top, bottom) = (rect.top + 80, rect.bottom - 40);
    println!(
        "before: top {:.2} bottom {:.2}",
        magenta(rect, top),
        magenta(rect, bottom)
    );
    for cycle in 0..cycles {
        // SAFETY: Our own live window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
        pump(probe);
        println!(
            "cycle {cycle}: +{probe} ms top {:.2} bottom {:.2}",
            magenta(rect, top),
            magenta(rect, bottom)
        );
        pump(1500);
        // SAFETY: Our own live window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        pump(1500);
    }
}
