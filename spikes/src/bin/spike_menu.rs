//! Renders the dock's context menu (the real `menu.rs` and `gfx.rs`) for a
//! moment, saves a screenshot of it and closes it: a visual check that needs
//! no clicks. Usage: `spike_menu <dark|light> <out.bgra>`; the file holds
//! width and height as two little-endian u32, then BGRA pixels.

#![allow(dead_code)]

#[path = "../../../apps/dock/src/gfx.rs"]
mod gfx;
#[path = "../../../apps/dock/src/menu.rs"]
mod menu;

use std::{io::Write, time::Duration};

use nosih_shell::screen;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, FindWindowW, GetWindowRect, PostMessageW,
                RegisterClassW, WM_CANCELMODE, WNDCLASSW, WS_POPUP,
            },
        },
    },
    core::w,
};

unsafe extern "system" fn proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dark = args.next().as_deref() != Some("light");
    let out = args.next().unwrap_or_else(|| "menu.bgra".into());
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // The compositor needs a dispatcher queue on this thread, as in the dock.
        let _queue = windows::Win32::System::WinRT::CreateDispatcherQueueController(
            windows::Win32::System::WinRT::DispatcherQueueOptions {
                dwSize: size_of::<windows::Win32::System::WinRT::DispatcherQueueOptions>() as u32,
                threadType: windows::Win32::System::WinRT::DQTYPE_THREAD_CURRENT,
                apartmentType: windows::Win32::System::WinRT::DQTAT_COM_STA,
            },
        )?;
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("SpikeMenuOwner"),
            ..Default::default()
        });
        let owner = CreateWindowExW(
            Default::default(),
            w!("SpikeMenuOwner"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let gfx = gfx::Gfx::new()?;
        let items = vec![
            menu::MenuItem::action(1, "Desactivar &ocultación automática"),
            menu::MenuItem::action(2, "Mostrar &escritorio"),
            menu::MenuItem::Action {
                command: 3,
                label: "&Mantener en el Dock".into(),
                checked: true,
                enabled: true,
                icon: None,
            },
            menu::MenuItem::Action {
                command: 4,
                label: "Abrir al iniciar sesión".into(),
                checked: false,
                enabled: false,
                icon: None,
            },
            menu::MenuItem::Separator,
            menu::MenuItem::action(5, "&Salir de Nosih"),
        ];
        let scenario = std::env::args().nth(3).unwrap_or_default();
        std::thread::spawn(move || {
            use windows::Win32::UI::WindowsAndMessaging::{
                WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
            };
            std::thread::sleep(Duration::from_millis(150));
            let Ok(menu) = FindWindowW(w!("NosihMenu"), None) else {
                return;
            };
            let at = |y: i32| LPARAM(((y as u32 & 0xFFFF) << 16 | 40) as isize);
            let post = |msg: u32, w: usize, l: LPARAM| {
                let _ = PostMessageW(Some(menu), msg, WPARAM(w), l);
            };
            if scenario.is_empty() {
                std::thread::sleep(Duration::from_millis(70));
                let mut rect = RECT::default();
                let _ = GetWindowRect(menu, &mut rect);
                let pad = 24;
                let area = RECT {
                    left: rect.left - pad,
                    top: rect.top - pad,
                    right: rect.right + pad,
                    bottom: rect.bottom + pad,
                };
                if let Some(shot) = screen::capture(area)
                    && let Ok(mut file) = std::fs::File::create(&out)
                {
                    let _ = file.write_all(&((area.right - area.left) as u32).to_le_bytes());
                    let _ = file.write_all(&((area.bottom - area.top) as u32).to_le_bytes());
                    let _ = file.write_all(&shot.bgra);
                }
                post(WM_CANCELMODE, 0, LPARAM(0));
                return;
            }
            // Past the delay that ignores the release of the opening click.
            std::thread::sleep(Duration::from_millis(350));
            match scenario.as_str() {
                "A" => {
                    post(WM_MOUSEMOVE, 0, at(50));
                    post(WM_LBUTTONDOWN, 1, at(50));
                    post(WM_LBUTTONUP, 0, at(50));
                }
                "B" => {
                    post(WM_MOUSEMOVE, 0, at(110));
                    post(WM_LBUTTONDOWN, 1, at(110));
                    post(WM_LBUTTONUP, 0, at(110));
                    std::thread::sleep(Duration::from_millis(200));
                    post(WM_KEYDOWN, 0x1B, LPARAM(0));
                }
                "C" => post(WM_KEYDOWN, 0x53, LPARAM(0)),
                "D" => post(WM_LBUTTONDOWN, 1, LPARAM(((5u32 << 16) | 0xFFEC) as isize)),
                "E" => {
                    for _ in 0..4 {
                        post(WM_KEYDOWN, 0x28, LPARAM(0));
                    }
                    post(WM_KEYDOWN, 0x0D, LPARAM(0));
                }
                _ => post(WM_CANCELMODE, 0, LPARAM(0)),
            }
            // Never leave a test menu open.
            std::thread::sleep(Duration::from_millis(1500));
            post(WM_CANCELMODE, 0, LPARAM(0));
        });
        let chosen = menu::show(
            &gfx,
            owner,
            &items,
            POINT { x: 960, y: 980 },
            dark,
            menu::Placement::Above,
        );
        println!("chosen: {chosen:?}");
    }
    Ok(())
}
