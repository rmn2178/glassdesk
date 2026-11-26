//! Minimizes a window and restores it with different methods, timing how
//! long it takes to be restored, in the foreground and on top of the other
//! app windows. Usage: `spike_restore <exe name> <method A..F> [calm]`.

use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM},
        Graphics::Dwm::{
            DWMWA_CLOAKED, DWMWA_TRANSITIONS_FORCEDISABLED, DwmGetWindowAttribute,
            DwmSetWindowAttribute,
        },
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, PWSTR},
};

fn exe(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buffer = [0u16; 512];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..len as usize])
            .rsplit(char::from(92))
            .next()
            .unwrap_or_default()
            .to_lowercase()
    }
}

struct Find {
    name: String,
    found: HWND,
}
unsafe extern "system" fn find(hwnd: HWND, l: LPARAM) -> BOOL {
    unsafe {
        let f = &mut *(l.0 as *mut Find);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        if (IsWindowVisible(hwnd).as_bool())
            && ex & WS_EX_TOOLWINDOW.0 == 0
            && GetWindowTextLengthW(hwnd) > 0
            && exe(hwnd) == f.name
        {
            f.found = hwnd;
            return false.into();
        }
        true.into()
    }
}

fn on_top(target: HWND) -> bool {
    unsafe {
        let mut frame = RECT::default();
        if IsIconic(target).as_bool() || GetWindowRect(target, &mut frame).is_err() {
            return false;
        }
        let mut current = GetTopWindow(None).ok();
        while let Some(w) = current {
            if w == target {
                return true;
            }
            let ex = GetWindowLongW(w, GWL_EXSTYLE) as u32;
            let mut r = RECT::default();
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(w, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4);
            if IsWindowVisible(w).as_bool()
                && !IsIconic(w).as_bool()
                && cloaked == 0
                && ex & (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0) == 0
                && GetWindowRect(w, &mut r).is_ok()
                && r.left < frame.right
                && frame.left < r.right
                && r.top < frame.bottom
                && frame.top < r.bottom
            {
                return false;
            }
            current = GetWindow(w, GW_HWNDNEXT).ok();
        }
        true
    }
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let method = std::env::args().nth(2).unwrap_or_else(|| "A".into());
    let calm = std::env::args().nth(3).as_deref() == Some("calm");
    unsafe {
        let mut f = Find {
            name,
            found: HWND::default(),
        };
        let _ = EnumWindows(Some(find), LPARAM(&raw mut f as isize));
        let hwnd = f.found;
        if hwnd.is_invalid() {
            println!("window not found");
            return;
        }
        let off = BOOL::from(calm);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const off).cast(),
            4,
        );
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        sleep(Duration::from_millis(1500));
        let start = Instant::now();
        match method.as_str() {
            "A" => {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            "B" => {
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_SYSCOMMAND,
                    WPARAM(SC_RESTORE as usize),
                    LPARAM(0),
                );
                let _ = SetForegroundWindow(hwnd);
            }
            "C" => SwitchToThisWindow(hwnd, true),
            "D" => {
                let _ = SetForegroundWindow(hwnd);
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            "E" => {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            "F" => {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_ASYNCWINDOWPOS,
                );
            }
            _ => {}
        }
        let call_ms = start.elapsed().as_millis();
        let (mut restored, mut front, mut top) = (None, None, None);
        while start.elapsed() < Duration::from_millis(4000) {
            let t = start.elapsed().as_millis();
            if restored.is_none() && !IsIconic(hwnd).as_bool() {
                restored = Some(t);
            }
            if front.is_none() && GetForegroundWindow() == hwnd {
                front = Some(t);
            }
            if top.is_none() && on_top(hwnd) {
                top = Some(t);
            }
            if restored.is_some() && front.is_some() && top.is_some() {
                break;
            }
            sleep(Duration::from_millis(5));
        }
        println!(
            "method {method} calm={calm}: call returned {call_ms} ms, restored {restored:?}, foreground {front:?}, on top {top:?}"
        );
        let on = BOOL::from(false);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const on).cast(),
            4,
        );
    }
}
