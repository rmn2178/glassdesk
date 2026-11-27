//! Click-through floating Liquid Glass HUD pill for volume and brightness feedback.

use anyhow::Result;
use nosih_shell::system;
use windows::{
    UI::{
        Color,
        Composition::{ContainerVisual, Desktop::DesktopWindowTarget},
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, IDC_ARROW, KillTimer,
            LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, WM_TIMER, WNDCLASSW,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::{BOOL, Interface, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    appearance,
    gfx::{Gfx, Ink},
    menubar::{add_rounded_box, add_visual, glyph},
    scene::Theme,
    strings,
};

const WIDTH: f32 = 216.0;
const HEIGHT: f32 = 52.0;
const BOTTOM_MARGIN: f32 = 96.0;
const HIDE_TIMER_ID: usize = 1;
const HIDE_DELAY_MS: u32 = 1_600;

/// Payload shown in the floating HUD pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Volume { percent: u8, muted: bool },
    Brightness { percent: u8 },
}

/// Click-through macOS-style floating Liquid Glass HUD pill.
pub struct Hud {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    surface: ContainerVisual,
    visible: bool,
}

impl Drop for Hud {
    fn drop(&mut self) {
        // SAFETY: `self.hwnd` is owned by this overlay for its lifetime.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), HIDE_TIMER_ID);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl Hud {
    /// Creates the hidden click-through DirectComposition HUD window.
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let class = w!("NosihHud");
        // SAFETY: Standard class registration and window creation on the UI
        // thread; the window is destroyed in `Drop`.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            // Registering an existing class fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(hud_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                class,
                w!("Nosih HUD"),
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
            // Without this, the system backdrop brush appears black.
            let enable = BOOL::from(true);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const enable).cast(),
                size_of::<BOOL>() as u32,
            );
            hwnd
        };
        let target = unsafe {
            // SAFETY: `hwnd` is the live window created above.
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        system::set_window_transitions(hwnd.0 as isize, false);
        let surface = gfx.compositor.CreateContainerVisual()?;
        target.SetRoot(&surface)?;

        Ok(Self {
            hwnd,
            _target: target,
            surface,
            visible: false,
        })
    }

    /// Renders and shows the HUD pill without activating it, arming a 1600ms auto-hide timer.
    pub fn show(&mut self, gfx: &Gfx, kind: Kind) -> Result<()> {
        let (screen_left, screen_top, screen_w, screen_h, work_bottom, scale) =
            primary_screen_metrics();
        let px = |logical: f32| (logical * scale).round();

        let phys_w = px(WIDTH);
        let phys_h = px(HEIGHT);
        let win_x = screen_left + ((screen_w - phys_w as i32) / 2);
        let preferred_y = work_bottom - phys_h as i32 - px(BOTTOM_MARGIN) as i32;
        let max_y = (screen_top + screen_h - phys_h as i32).max(screen_top);
        let win_y = preferred_y.clamp(screen_top, max_y);

        self.render_visuals(gfx, kind, phys_w, phys_h, scale)?;

        // SAFETY: `self.hwnd` is our own live click-through popup window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                win_x,
                win_y,
                phys_w as i32,
                phys_h as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetTimer(Some(self.hwnd), HIDE_TIMER_ID, HIDE_DELAY_MS, None);
        }
        self.visible = true;
        Ok(())
    }

    /// Cancels the auto-hide timer, hides the HUD window, and clears its visual tree.
    pub fn hide(&mut self) -> Result<()> {
        // SAFETY: `self.hwnd` is our own live window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), HIDE_TIMER_ID);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if self.visible {
            self.surface.Children()?.RemoveAll()?;
            self.visible = false;
        }
        Ok(())
    }

    /// Handles the one-shot auto-hide timer callback.
    pub fn on_timer(&mut self) -> Result<()> {
        self.hide()
    }

    fn render_visuals(
        &self,
        gfx: &Gfx,
        kind: Kind,
        phys_w: f32,
        phys_h: f32,
        scale: f32,
    ) -> Result<()> {
        let c = &gfx.compositor;
        let px = |logical: f32| (logical * scale).round();
        let look = appearance::current();
        let theme = Theme::for_popup_look(look);
        let ink = if look.light { Ink::Dark } else { Ink::Light };
        let t = strings::text();

        self.surface.SetSize(Vector2::new(phys_w, phys_h))?;
        let layers = self.surface.Children()?;
        layers.RemoveAll()?;

        let panel = theme.panel(gfx, Vector2::new(phys_w, phys_h), phys_h / 2.0, scale)?;
        layers.InsertAtTop(&panel)?;

        let content = c.CreateContainerVisual()?;
        content.SetSize(Vector2::new(phys_w, phys_h))?;
        let children = content.Children()?;

        let (glyph_ch, label, raw_percent, muted) = match kind {
            Kind::Volume { percent, muted } => (
                volume_glyph(percent, muted),
                if muted { t.hud_muted } else { t.hud_volume },
                percent,
                muted,
            ),
            Kind::Brightness { percent } => (glyph::BRIGHTNESS, t.hud_brightness, percent, false),
        };
        let percent = clamp_percent(raw_percent);

        // Leading icon glyph.
        let icon = gfx.glyph_brush(&[(glyph_ch, 0.92)], scale, ink)?;
        add_visual(
            c,
            &children,
            &icon.brush.cast()?,
            px(16.0),
            (phys_h - icon.height) / 2.0,
            icon.width,
            icon.height,
        )?;

        let body_x = px(44.0);
        let right_pad = px(18.0);
        let body_w = (phys_w - body_x - right_pad).max(px(40.0));

        // Top row: label on the left, `{percent}%` on the right.
        let pct_text = format!("{percent}%");
        let pct_lbl = gfx.text_brush_sized(&pct_text, scale, 11.5, true, ink)?;
        let label_max_w = (body_w - pct_lbl.width - px(8.0)).max(px(24.0));
        let title_lbl = gfx.text_brush_fitted(label, scale, 11.5, true, ink, label_max_w)?;

        let top_y = px(10.0);
        add_visual(
            c,
            &children,
            &title_lbl.brush.cast()?,
            body_x,
            top_y,
            title_lbl.width,
            title_lbl.height,
        )?;
        add_visual(
            c,
            &children,
            &pct_lbl.brush.cast()?,
            body_x + body_w - pct_lbl.width,
            top_y,
            pct_lbl.width,
            pct_lbl.height,
        )?
        .SetOpacity(0.78)?;

        // Bottom row: smooth rounded track and level fill bar.
        let bar_h = px(6.0).max(4.0);
        let bar_y = phys_h - px(13.0) - bar_h;
        let bar_radius = bar_h / 2.0;
        let track_color = if look.light {
            rgba(0, 0, 0, 34)
        } else {
            rgba(255, 255, 255, 42)
        };
        add_rounded_box(
            c,
            &children,
            body_x,
            bar_y,
            body_w,
            bar_h,
            bar_radius,
            track_color,
            None,
        )?;

        let fill_w = bar_fill_width(body_w, percent, muted);
        if fill_w > 0.0 {
            let fill_color = if look.light {
                rgba(16, 18, 22, 224)
            } else {
                rgba(255, 255, 255, 238)
            };
            add_rounded_box(
                c,
                &children,
                body_x,
                bar_y,
                fill_w,
                bar_h,
                bar_radius,
                fill_color,
                None,
            )?;
        }

        content.SetOffset(Vector3::new(0.0, 0.0, 0.0))?;
        layers.InsertAtTop(&content)?;
        Ok(())
    }
}

/// Clamps any raw percentage into `0..=100`.
#[must_use]
fn clamp_percent(percent: u8) -> u8 {
    percent.min(100)
}

/// Computes the filled bar width for a given track width, percentage, and mute state.
#[must_use]
fn bar_fill_width(track_width: f32, percent: u8, muted: bool) -> f32 {
    if muted || track_width <= 0.0 || !track_width.is_finite() {
        return 0.0;
    }
    let clamped = clamp_percent(percent);
    if clamped == 0 {
        return 0.0;
    }
    (track_width * f32::from(clamped) / 100.0).clamp(0.0, track_width)
}

/// Picks the matching Segoe Fluent Icons volume glyph for `percent` and `muted`.
#[must_use]
fn volume_glyph(percent: u8, muted: bool) -> char {
    if muted {
        return glyph::MUTE;
    }
    match clamp_percent(percent) {
        0 => glyph::VOLUME[0],
        1..=33 => glyph::VOLUME[1],
        34..=66 => glyph::VOLUME[2],
        _ => glyph::VOLUME[3],
    }
}

fn primary_screen_metrics() -> (i32, i32, i32, i32, i32, f32) {
    if let Some(monitor) = system::primary_monitor() {
        let w = (monitor.screen.right - monitor.screen.left).max(1);
        let h = (monitor.screen.bottom - monitor.screen.top).max(1);
        (
            monitor.screen.left,
            monitor.screen.top,
            w,
            h,
            monitor.work_bottom,
            monitor.scale.max(0.5),
        )
    } else {
        (0, 0, 1920, 1080, 1040, 1.0)
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

unsafe extern "system" fn hud_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if w.0 == HIDE_TIMER_ID => {
            crate::app::hud_timeout();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_clamping_caps_at_hundred() {
        assert_eq!(clamp_percent(0), 0);
        assert_eq!(clamp_percent(42), 42);
        assert_eq!(clamp_percent(100), 100);
        assert_eq!(clamp_percent(120), 100);
        assert_eq!(clamp_percent(u8::MAX), 100);
    }

    #[test]
    fn bar_fill_width_handles_zero_muted_full_and_invalid_tracks() {
        assert_eq!(bar_fill_width(150.0, 0, false), 0.0);
        assert_eq!(bar_fill_width(150.0, 75, true), 0.0);
        assert_eq!(bar_fill_width(0.0, 50, false), 0.0);
        assert_eq!(bar_fill_width(-10.0, 50, false), 0.0);
        assert_eq!(bar_fill_width(f32::NAN, 50, false), 0.0);

        assert!((bar_fill_width(200.0, 50, false) - 100.0).abs() < 1e-4);
        assert!((bar_fill_width(200.0, 100, false) - 200.0).abs() < 1e-4);
        assert!((bar_fill_width(200.0, 180, false) - 200.0).abs() < 1e-4);
    }

    #[test]
    fn volume_glyph_tracks_mute_and_level_buckets() {
        assert_eq!(volume_glyph(80, true), glyph::MUTE);
        assert_eq!(volume_glyph(0, false), glyph::VOLUME[0]);
        assert_eq!(volume_glyph(20, false), glyph::VOLUME[1]);
        assert_eq!(volume_glyph(50, false), glyph::VOLUME[2]);
        assert_eq!(volume_glyph(100, false), glyph::VOLUME[3]);
    }
}
