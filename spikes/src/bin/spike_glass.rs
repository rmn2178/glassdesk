//! Prototypes (a) and (c): rounded glass showing the desktop backdrop and an
//! input region that lets clicks outside the dock outline pass through.
//!
//! Usage: `spike_glass [seconds]` (15 by default). Prints click-test results
//! and exits automatically.

use std::env;

use windows_numerics::{Vector2, Vector3};

use windows::{
    System::DispatcherQueueController,
    UI::{
        Color,
        Composition::{Compositor, ContainerVisual, Desktop::DesktopWindowTarget},
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::{
            Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
            Gdi::{CreateRoundRectRgn, SetWindowRgn},
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::{
                Composition::ICompositorDesktopInterop, CreateDispatcherQueueController,
                DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
            },
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
                KillTimer, MSG, PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN,
                SW_SHOWNOACTIVATE, SetTimer, ShowWindow, WM_DESTROY, WM_TIMER, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_POPUP, WindowFromPoint,
            },
        },
    },
    core::{BOOL, Interface, Result, w},
};

// The window is taller than the dock to leave room for magnification above it.
const WIN_W: i32 = 760;
const WIN_H: i32 = 220;
const PILL_W: i32 = 640;
const PILL_H: i32 = 72;
const PILL_RADIUS: f32 = 26.0;
fn margin_bottom() -> i32 {
    env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
}

const TIMER_HIT_TEST: usize = 1;
const TIMER_QUIT: usize = 2;

fn pill_origin() -> (i32, i32) {
    ((WIN_W - PILL_W) / 2, WIN_H - PILL_H - 8)
}

fn rgba(a: u8, r: u8, g: u8, b: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

/// Visual tree: clipped glass backdrop, tint, bright border, and a control
/// square outside the outline to check whether the input region clips it.
fn build_visuals(compositor: &Compositor) -> Result<ContainerVisual> {
    let (px, py) = pill_origin();
    let size = Vector2 {
        X: PILL_W as f32,
        Y: PILL_H as f32,
    };
    let offset = Vector3 {
        X: px as f32,
        Y: py as f32,
        Z: 0.0,
    };

    let root = compositor.CreateContainerVisual()?;
    root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;

    let pill = compositor.CreateContainerVisual()?;
    pill.SetSize(size)?;
    pill.SetOffset(offset)?;
    let geometry = compositor.CreateRoundedRectangleGeometry()?;
    geometry.SetSize(size)?;
    geometry.SetCornerRadius(Vector2 {
        X: PILL_RADIUS,
        Y: PILL_RADIUS,
    })?;
    pill.SetClip(&compositor.CreateGeometricClipWithGeometry(&geometry)?)?;

    let backdrop = compositor.CreateSpriteVisual()?;
    backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    backdrop.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
    pill.Children()?.InsertAtBottom(&backdrop)?;

    let tint = compositor.CreateSpriteVisual()?;
    tint.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    tint.SetBrush(&compositor.CreateColorBrushWithColor(rgba(46, 255, 255, 255))?)?;
    pill.Children()?.InsertAtTop(&tint)?;
    root.Children()?.InsertAtTop(&pill)?;

    let rim_geometry = compositor.CreateRoundedRectangleGeometry()?;
    rim_geometry.SetSize(Vector2 {
        X: size.X - 1.0,
        Y: size.Y - 1.0,
    })?;
    rim_geometry.SetOffset(Vector2 { X: 0.5, Y: 0.5 })?;
    rim_geometry.SetCornerRadius(Vector2 {
        X: PILL_RADIUS,
        Y: PILL_RADIUS,
    })?;
    let rim = compositor.CreateSpriteShapeWithGeometry(&rim_geometry)?;
    rim.SetStrokeBrush(&compositor.CreateColorBrushWithColor(rgba(110, 255, 255, 255))?)?;
    rim.SetStrokeThickness(1.0)?;
    let rim_visual = compositor.CreateShapeVisual()?;
    rim_visual.SetSize(size)?;
    rim_visual.SetOffset(offset)?;
    rim_visual.Shapes()?.Append(&rim)?;
    root.Children()?.InsertAtTop(&rim_visual)?;

    for i in 0..9 {
        let icon = compositor.CreateSpriteVisual()?;
        icon.SetSize(Vector2 { X: 52.0, Y: 52.0 })?;
        icon.SetOffset(Vector3 {
            X: px as f32 + 20.0 + i as f32 * 68.0,
            Y: py as f32 + 10.0,
            Z: 0.0,
        })?;
        let hue = (i * 20) as u8;
        icon.SetBrush(&compositor.CreateColorBrushWithColor(rgba(
            255,
            255 - hue,
            90 + hue / 2,
            60 + hue,
        ))?)?;
        root.Children()?.InsertAtTop(&icon)?;
    }

    let control = compositor.CreateSpriteVisual()?;
    control.SetSize(Vector2 { X: 40.0, Y: 40.0 })?;
    control.SetOffset(Vector3 {
        X: 10.0,
        Y: 10.0,
        Z: 0.0,
    })?;
    control.SetBrush(&compositor.CreateColorBrushWithColor(rgba(255, 255, 0, 0))?)?;
    root.Children()?.InsertAtTop(&control)?;

    Ok(root)
}

fn hit_test(hwnd: HWND, origin: POINT) {
    let (px, py) = pill_origin();
    let cases = [
        ("centro del dock", px + PILL_W / 2, py + PILL_H / 2, true),
        (
            "zona transparente sobre el dock",
            px + PILL_W / 2,
            py - 60,
            false,
        ),
        (
            "esquina redondeada (fuera de la curva)",
            px + 2,
            py + 2,
            false,
        ),
    ];
    for (name, x, y, expect_ours) in cases {
        let point = POINT {
            x: origin.x + x,
            y: origin.y + y,
        };
        // SAFETY: Read-only query.
        let ours = unsafe { WindowFromPoint(point) } == hwnd;
        let verdict = if ours == expect_ours { "OK" } else { "FALLO" };
        println!(
            "[{verdict}] {name}: el clic llega a {}",
            if ours {
                "Nosih"
            } else {
                "la ventana de abajo"
            }
        );
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if w.0 == TIMER_HIT_TEST => {
            // SAFETY: `hwnd` is our live window.
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HIT_TEST);
            }
            hit_test(hwnd, window_origin());
            LRESULT(0)
        }
        WM_TIMER if w.0 == TIMER_QUIT => {
            // SAFETY: Ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: Same as above.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: Standard message forwarding.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn window_origin() -> POINT {
    // SAFETY: Read-only metric queries.
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    POINT {
        x: (sw - WIN_W) / 2,
        y: sh - WIN_H - margin_bottom(),
    }
}

fn main() -> Result<()> {
    let seconds: u32 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);

    // SAFETY: Single-threaded Win32/WinRT initialization; all passed pointers
    // remain valid for the duration of the call.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;

        // The compositor requires a dispatcher queue on this thread.
        let _queue: DispatcherQueueController =
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?;

        let instance = GetModuleHandleW(None)?;
        let class = w!("NosihSpikeGlass");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });

        let origin = window_origin();
        let hwnd = CreateWindowExW(
            // NOACTIVATE keeps the dock from stealing focus while the glass remains active.
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Nosih spike"),
            WS_POPUP,
            origin.x,
            origin.y,
            WIN_W,
            WIN_H,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        let enable = BOOL::from(true);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            (&raw const enable).cast(),
            size_of::<BOOL>() as u32,
        )?;

        let compositor = Compositor::new()?;
        let target: DesktopWindowTarget = compositor
            .cast::<ICompositorDesktopInterop>()?
            .CreateDesktopWindowTarget(hwnd, true)?;
        target.SetRoot(&build_visuals(&compositor)?)?;

        // (c) Match the input region to the dock outline; ownership transfers to the system.
        let (px, py) = pill_origin();
        let d = (PILL_RADIUS * 2.0) as i32;
        let region = CreateRoundRectRgn(px, py, px + PILL_W + 1, py + PILL_H + 1, d, d);
        SetWindowRgn(hwnd, Some(region), true);

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), TIMER_HIT_TEST, 1000, None);
        SetTimer(Some(hwnd), TIMER_QUIT, seconds * 1000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
