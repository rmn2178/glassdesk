//! Prototype (b): hide and restore the Windows taskbar.
//!
//! Usage: `spike_taskbar test | hide | restore | watch`
//! - `test`: hides the taskbar, samples its state for 4 seconds, then restores it.
//!
//! Finding: `ShowWindow(SW_HIDE)` is insufficient on Windows 11 because Explorer
//! shows the taskbar again. Instead, auto-hide is combined with layered-window
//! styles that make the taskbar invisible and click-through.
//! - `watch`: hides the taskbar and hides it again if Explorer restarts. Ctrl+C
//!   does not restore it; use `restore` for that.

use std::{env, mem::size_of, thread::sleep, time::Duration};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW,
                GWL_EXSTYLE, GetMessageW, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
                LWA_ALPHA, MSG, RegisterClassW, RegisterWindowMessageW, SPI_GETWORKAREA,
                SetLayeredWindowAttributes, SetWindowLongPtrW, SystemParametersInfoW,
                WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const PRIMARY: PCWSTR = w!("Shell_TrayWnd");
const SECONDARY: PCWSTR = w!("Shell_SecondaryTrayWnd");

fn taskbars() -> Vec<HWND> {
    let mut found = Vec::new();
    // SAFETY: Read-only calls with valid static strings.
    unsafe {
        if let Ok(hwnd) = FindWindowW(PRIMARY, None) {
            found.push(hwnd);
        }
        let mut prev = None;
        while let Ok(hwnd) = FindWindowExW(None, prev, SECONDARY, None) {
            found.push(hwnd);
            prev = Some(hwnd);
        }
    }
    found
}

fn appbar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: taskbars().first().copied().unwrap_or_default(),
        ..Default::default()
    }
}

fn autohide_enabled() -> bool {
    let mut data = appbar_data();
    // SAFETY: `data` is a valid APPBARDATA with `cbSize` initialized.
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    state as u32 & ABS_AUTOHIDE != 0
}

fn set_autohide(enabled: bool) {
    let mut data = appbar_data();
    data.lParam = LPARAM(if enabled { ABS_AUTOHIDE as isize } else { 0 });
    // SAFETY: Same as above.
    unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
}

const GHOST_STYLES: isize = (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize;

fn set_ghost(hwnd: HWND, ghost: bool) {
    // SAFETY: `hwnd` comes from FindWindow; calls have no effect if it no longer exists.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ghost {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | GHOST_STYLES);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        } else {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !GHOST_STYLES);
        }
    }
}

fn hide() {
    // Auto-hide frees the work area; ghost mode prevents the taskbar from
    // appearing or receiving clicks when the cursor reaches the screen edge.
    set_autohide(true);
    for hwnd in taskbars() {
        set_ghost(hwnd, true);
    }
}

fn restore(autohide: bool) {
    for hwnd in taskbars() {
        set_ghost(hwnd, false);
    }
    set_autohide(autohide);
}

fn report(step: &str) {
    let mut work = RECT::default();
    // SAFETY: `work` is a valid RECT for SPI_GETWORKAREA to write to.
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast()),
            Default::default(),
        );
    }
    let visible: Vec<(bool, isize, i32)> = taskbars()
        .into_iter()
        .map(|hwnd| {
            let mut rect = RECT::default();
            // SAFETY: Read-only queries.
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
                let ghost = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & GHOST_STYLES;
                (IsWindowVisible(hwnd).as_bool(), ghost, rect.top)
            }
        })
        .collect();
    println!(
        "{step:<22} barras(visible,fantasma,top)={visible:?} autoocultar={} area_trabajo=({},{})-({},{})",
        autohide_enabled(),
        work.left,
        work.top,
        work.right,
        work.bottom
    );
}

fn watch() -> Result<()> {
    static TASKBAR_CREATED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if Some(&msg) == TASKBAR_CREATED.get() {
            println!("TaskbarCreated received: Explorer restarted; hiding the taskbar again");
            // The new taskbar needs a moment to apply its initial state.
            sleep(Duration::from_millis(500));
            hide();
            report("tras reinicio");
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    // SAFETY: Standard class registration and message loop on a single thread.
    unsafe {
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let instance = GetModuleHandleW(None)?;
        let class = w!("NosihSpikeTaskbarWatch");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });
        // TaskbarCreated is broadcast only to top-level windows, not message-only
        // windows, so create a normal window without showing it.
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        hide();
        report("oculta (watch)");
        println!("Restart Explorer from Task Manager to test.");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("test") => {
            let original = autohide_enabled();
            report("antes");
            hide();
            for i in 1..=4 {
                sleep(Duration::from_secs(1));
                report(&format!("oculta (+{i} s)"));
            }
            restore(original);
            sleep(Duration::from_secs(1));
            report("restaurada");
        }
        Some("hide") => {
            hide();
            report("oculta");
        }
        Some("restore") => {
            restore(false);
            report("restaurada");
        }
        Some("watch") => watch()?,
        _ => eprintln!("uso: spike_taskbar test | hide | restore | watch"),
    }
    Ok(())
}
