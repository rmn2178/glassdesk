//! Checks how the menu bar tells a window's minimize/maximize/close buttons
//! apart: DWM's caption button bounds and the window's own WM_NCHITTEST.
//! Only queries; nothing is clicked.

use std::{thread::sleep, time::Duration};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
                GetClassNameW, GetWindowRect, GetWindowTextW, IsWindowVisible, MSG, PM_REMOVE,
                PeekMessageW, RegisterClassW, SMTO_ABORTIFHUNG, SW_MAXIMIZE, SendMessageTimeoutW,
                ShowWindow, WM_NCHITTEST, WNDCLASSW, WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::{BOOL, w},
};

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn report(name: &str, window: HWND, point: POINT) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
        let under = GetAncestor(WindowFromPoint(point), GA_ROOT);
        let mut class = [0u16; 64];
        let n = GetClassNameW(under, &mut class) as usize;
        let mut r = RECT::default();
        let _ = GetWindowRect(under, &mut r);
        println!(
            "  under {point:?}: {} {r:?} same={}",
            String::from_utf16_lossy(&class[..n]),
            under == window
        );
        let mut frame = RECT::default();
        let _ = GetWindowRect(window, &mut frame);
        let mut buttons = RECT::default();
        let dwm = DwmGetWindowAttribute(
            window,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            (&raw mut buttons).cast(),
            size_of::<RECT>() as u32,
        );
        let position = ((point.y as u32 & 0xFFFF) << 16 | (point.x as u32 & 0xFFFF)) as isize;
        let mut hit = 0usize;
        let answered = SendMessageTimeoutW(
            window,
            WM_NCHITTEST,
            WPARAM(0),
            LPARAM(position),
            SMTO_ABORTIFHUNG,
            50,
            Some(&mut hit),
        );
        let screen = RECT {
            left: 0,
            top: frame.top + 9,
            right: 1920,
            bottom: 1080,
        };
        println!(
            "{name}: caption_button_at={} frame={frame:?} point={point:?} dwm buttons={:?} nchittest={} (8 min, 9 max, 20 close)",
            nosih_shell::tracker::caption_button_at(point, screen),
            dwm.map(|()| buttons),
            if answered.0 != 0 {
                hit.to_string()
            } else {
                "no answer".into()
            }
        );
    }
}

unsafe extern "system" fn find_brave(hwnd: HWND, out: LPARAM) -> BOOL {
    unsafe {
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let mut title = [0u16; 256];
        let t = GetWindowTextW(hwnd, &mut title) as usize;
        if IsWindowVisible(hwnd).as_bool()
            && String::from_utf16_lossy(&class[..n]) == "Chrome_WidgetWin_1"
            && String::from_utf16_lossy(&title[..t]).contains("Brave")
        {
            *(out.0 as *mut HWND) = hwnd;
            return false.into();
        }
        true.into()
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("NosihSpikeCaption"),
            ..Default::default()
        });
        let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            w!("NosihSpikeCaption"),
            w!("caption"),
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
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        let mut msg = MSG::default();
        for _ in 0..60 {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            sleep(Duration::from_millis(10));
        }
        let mut frame = RECT::default();
        let _ = GetWindowRect(hwnd, &mut frame);
        // The top-right pixel of the screen area the maximized window shows.
        let corner = POINT {
            x: frame.right - 10,
            y: frame.top + 9,
        };
        report("test window, top-right corner", hwnd, corner);
        report(
            "test window, left of the buttons",
            hwnd,
            POINT {
                x: corner.x - 200,
                y: corner.y,
            },
        );
        report(
            "test window, middle of title",
            hwnd,
            POINT {
                x: (frame.left + frame.right) / 2,
                y: frame.top + 9,
            },
        );
        let _ = DestroyWindow(hwnd);

        let mut brave = HWND::default();
        let _ = EnumWindows(Some(find_brave), LPARAM(&raw mut brave as isize));
        if !brave.is_invalid() {
            let mut frame = RECT::default();
            let _ = GetWindowRect(brave, &mut frame);
            report(
                "brave, close button",
                brave,
                POINT {
                    x: frame.right - 30,
                    y: frame.top + 15,
                },
            );
            report(
                "brave, a tab",
                brave,
                POINT {
                    x: frame.left + 200,
                    y: frame.top + 15,
                },
            );
        }
    }
}
