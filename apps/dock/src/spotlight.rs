//! Floating Liquid Glass Spotlight search overlay for quick app launching,
//! calculator/unit evaluation, and system commands.

use std::borrow::Cow;

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use nosih_shell::{
    spotlight::{Entry, EntryKind, Index},
    system,
};

static SPOTLIGHT_OPEN: AtomicBool = AtomicBool::new(false);
use windows::{
    Foundation::TimeSpan,
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
        UI::{
            Input::KeyboardAndMouse::{SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, IDC_ARROW,
                LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetCursor,
                SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CHAR,
                WM_KEYDOWN, WM_LBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::{BOOL, Interface, h, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    appearance,
    gfx::{Gfx, Ink},
    menubar::{add_rounded_box, add_visual, glyph},
    scene::Theme,
    strings,
};

const WIDTH: f32 = 620.0;
const INPUT_HEIGHT: f32 = 64.0;
const LIST_TOP_GAP: f32 = 8.0;
const ROW_HEIGHT: f32 = 48.0;
const LIST_BOTTOM_PAD: f32 = 12.0;
const MAX_RESULTS: usize = 7;
const PANEL_RADIUS: f32 = 18.0;
const ROW_INSET_X: f32 = 10.0;
const ROW_RADIUS: f32 = 10.0;
const PAD_X: f32 = 10.0;
const PAD_TOP: f32 = 14.0;
const PAD_BOTTOM: f32 = 10.0;

/// Centered floating Liquid Glass Spotlight search overlay.
pub struct Spotlight {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    surface: ContainerVisual,
    index: Option<Index>,
    query: String,
    results: Vec<Entry>,
    selected: usize,
    scale: f32,
    open: bool,
}

impl Drop for Spotlight {
    fn drop(&mut self) {
        SPOTLIGHT_OPEN.store(false, Ordering::Relaxed);
        // SAFETY: `self.hwnd` is owned by this overlay for its lifetime.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl Spotlight {
    /// Creates the hidden DirectComposition Spotlight overlay window.
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let class = w!("NosihSpotlight");
        // SAFETY: Standard class registration and window creation on the UI
        // thread; the window is destroyed in `Drop`.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            // Registering an existing class fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(spotlight_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class,
                w!("Nosih Spotlight"),
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
        let root = gfx.compositor.CreateContainerVisual()?;
        let surface = gfx.compositor.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&surface)?;
        target.SetRoot(&root)?;

        Ok(Self {
            hwnd,
            _target: target,
            surface,
            index: None,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            scale: 1.0,
            open: false,
        })
    }

    /// Toggles the Spotlight overlay open or closed.
    pub fn toggle(&mut self, gfx: &Gfx) -> Result<()> {
        if self.open {
            self.close()
        } else {
            self.open(gfx)
        }
    }

    /// Opens the Spotlight overlay, lazily building the app index on first use.
    pub fn open(&mut self, gfx: &Gfx) -> Result<()> {
        self.query.clear();
        self.selected = 0;
        let index = self.index.get_or_insert_with(Index::build);
        self.results = index.search(&self.query, MAX_RESULTS);
        self.open = true;
        SPOTLIGHT_OPEN.store(true, Ordering::Relaxed);
        self.paint_and_position(gfx, true)?;
        // SAFETY: `self.hwnd` is our own live top-level popup window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
        Ok(())
    }

    /// Hides the Spotlight overlay and releases its visual children.
    pub fn close(&mut self) -> Result<()> {
        self.open = false;
        SPOTLIGHT_OPEN.store(false, Ordering::Relaxed);
        self.query.clear();
        self.results.clear();
        self.selected = 0;
        self.surface.Children()?.RemoveAll()?;
        self.reset_surface_transform(0.0, 0.0)?;
        // SAFETY: `self.hwnd` is our own live top-level popup window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        Ok(())
    }

    /// Handles character input (`WM_CHAR`) while Spotlight is open.
    pub fn on_char(&mut self, gfx: &Gfx, ch: char) -> Result<Option<Entry>> {
        if !self.open {
            return Ok(None);
        }
        match ch {
            '\u{8}' => {
                if self.query.pop().is_some() {
                    self.recompute_results();
                    self.paint_and_position(gfx, false)?;
                }
            }
            '\u{1b}' => {
                self.close()?;
            }
            '\r' => {
                return self.take_selected();
            }
            c if !c.is_control() => {
                self.query.push(c);
                self.recompute_results();
                self.paint_and_position(gfx, false)?;
            }
            _ => {}
        }
        Ok(None)
    }

    /// Handles navigation and command keys (`WM_KEYDOWN`) while Spotlight is open.
    pub fn on_key_down(&mut self, gfx: &Gfx, vk: u16) -> Result<Option<Entry>> {
        if !self.open {
            return Ok(None);
        }
        if vk == VK_UP.0 {
            let next = move_selection(self.selected, self.results.len(), -1);
            if next != self.selected {
                self.selected = next;
                self.paint_and_position(gfx, false)?;
            }
        } else if vk == VK_DOWN.0 {
            let next = move_selection(self.selected, self.results.len(), 1);
            if next != self.selected {
                self.selected = next;
                self.paint_and_position(gfx, false)?;
            }
        } else if vk == VK_ESCAPE.0 {
            self.close()?;
        } else if vk == VK_RETURN.0 {
            return self.take_selected();
        }
        Ok(None)
    }

    /// Handles a left-click in client coordinates, returning any clicked result row to execute.
    pub fn on_click(&mut self, _gfx: &Gfx, x: f32, y: f32) -> Result<Option<Entry>> {
        if !self.open {
            return Ok(None);
        }
        let local_x = x - (PAD_X * self.scale).round();
        let local_y = y - (PAD_TOP * self.scale).round();
        let max_x = WIDTH * self.scale;
        if local_x < 0.0 || local_x > max_x {
            return Ok(None);
        }
        if let Some(index) = hit_test_row(local_y, self.scale, self.results.len()) {
            self.selected = index;
            return self.take_selected();
        }
        Ok(None)
    }

    /// Closes Spotlight when the window loses activation (`WA_INACTIVE`).
    pub fn on_deactivate(&mut self) -> Result<()> {
        if self.open {
            self.close()?;
        }
        Ok(())
    }

    /// Repaints Spotlight with the updated system/app appearance if open.
    pub fn on_theme_change(&mut self, gfx: &Gfx) -> Result<()> {
        if self.open {
            self.paint_and_position(gfx, false)?;
        }
        Ok(())
    }

    fn recompute_results(&mut self) {
        let index = self.index.get_or_insert_with(Index::build);
        self.results = index.search(&self.query, MAX_RESULTS);
        self.selected = 0;
    }

    fn take_selected(&mut self) -> Result<Option<Entry>> {
        let chosen = self.results.get(self.selected).cloned();
        self.close()?;
        Ok(chosen)
    }

    fn paint_and_position(&mut self, gfx: &Gfx, show: bool) -> Result<()> {
        let (screen_left, screen_top, screen_w, screen_h, scale) = primary_screen_metrics();
        self.scale = scale;
        let px = |logical: f32| (logical * scale).round();

        let has_query = !self.query.trim().is_empty();
        let logical_height = panel_logical_height(self.results.len(), has_query);
        let phys_w = px(WIDTH);
        let phys_h = px(logical_height);
        let pad_x = px(PAD_X);
        let pad_top = px(PAD_TOP);
        let pad_bottom = px(PAD_BOTTOM);
        let win_w = (phys_w + pad_x * 2.0) as i32;
        let win_h = (phys_h + pad_top + pad_bottom) as i32;

        let win_x = screen_left + ((screen_w - phys_w as i32) / 2) - pad_x as i32;
        let preferred_y = screen_top + ((screen_h as f32 * 0.22).round() as i32) - pad_top as i32;
        let max_y = (screen_top + screen_h - win_h).max(screen_top);
        let win_y = preferred_y.clamp(screen_top, max_y);

        self.render_visuals(gfx, phys_w, phys_h, scale, has_query)?;
        if show {
            if system::animations_enabled() {
                self.animate_entrance(gfx, phys_w, pad_x, pad_top, scale)?;
            } else {
                self.reset_surface_transform(pad_x, pad_top)?;
            }
        } else {
            self.surface.SetOffset(Vector3::new(pad_x, pad_top, 0.0))?;
        }
        let flags = if show {
            SWP_SHOWWINDOW
        } else {
            SWP_NOACTIVATE
        };
        // SAFETY: `self.hwnd` is our own live top-level popup window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                win_x,
                win_y,
                win_w,
                win_h,
                flags,
            );
        }
        Ok(())
    }

    fn render_visuals(
        &self,
        gfx: &Gfx,
        phys_w: f32,
        phys_h: f32,
        scale: f32,
        has_query: bool,
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

        let panel = theme.panel(gfx, Vector2::new(phys_w, phys_h), px(PANEL_RADIUS), scale)?;
        layers.InsertAtTop(&panel)?;

        let content = c.CreateContainerVisual()?;
        content.SetSize(Vector2::new(phys_w, phys_h))?;
        let children = content.Children()?;

        // Search glyph and query / placeholder input row.
        let input_h = px(INPUT_HEIGHT);
        let search_icon = gfx.glyph_brush(&[(glyph::SEARCH, 0.85)], scale, ink)?;
        add_visual(
            c,
            &children,
            &search_icon.brush.cast()?,
            px(20.0),
            (input_h - search_icon.height) / 2.0,
            search_icon.width,
            search_icon.height,
        )?;

        let input_x = px(52.0);
        let input_max_w = (phys_w - input_x - px(20.0)).max(px(40.0));
        let (display_text, text_opacity) = if self.query.is_empty() {
            (t.spotlight_placeholder, 0.48)
        } else {
            (self.query.as_str(), 0.96)
        };
        let query_lbl = gfx.text_brush_fitted(display_text, scale, 20.0, false, ink, input_max_w)?;
        add_visual(
            c,
            &children,
            &query_lbl.brush.cast()?,
            input_x,
            (input_h - query_lbl.height) / 2.0,
            query_lbl.width,
            query_lbl.height,
        )?
        .SetOpacity(text_opacity)?;

        // Subtle divider + result rows or empty-state message.
        if !self.results.is_empty() || has_query {
            let divider_color = if look.light {
                rgba(0, 0, 0, 26)
            } else {
                rgba(255, 255, 255, 32)
            };
            add_visual(
                c,
                &children,
                &c.CreateColorBrushWithColor(divider_color)?.cast()?,
                px(14.0),
                input_h,
                (phys_w - px(28.0)).max(1.0),
                px(1.0).max(1.0),
            )?;
        }

        if self.results.is_empty() && has_query {
            let empty_y = px(INPUT_HEIGHT + LIST_TOP_GAP);
            let empty_h = px(ROW_HEIGHT);
            let empty_lbl = gfx.text_brush_fitted(
                t.spotlight_empty,
                scale,
                14.0,
                false,
                ink,
                (phys_w - px(48.0)).max(px(60.0)),
            )?;
            add_visual(
                c,
                &children,
                &empty_lbl.brush.cast()?,
                (phys_w - empty_lbl.width) / 2.0,
                empty_y + (empty_h - empty_lbl.height) / 2.0,
                empty_lbl.width,
                empty_lbl.height,
            )?
            .SetOpacity(0.62)?;
        } else if !self.results.is_empty() {
            let highlight_fill = if look.light {
                rgba(0, 122, 255, 36)
            } else {
                rgba(255, 255, 255, 34)
            };
            let highlight_border = if look.light {
                rgba(0, 122, 255, 76)
            } else {
                rgba(255, 255, 255, 58)
            };
            let row_w = (phys_w - px(ROW_INSET_X * 2.0)).max(1.0);
            let row_h = px(ROW_HEIGHT);

            for (idx, entry) in self.results.iter().take(MAX_RESULTS).enumerate() {
                let row_y = px(INPUT_HEIGHT + LIST_TOP_GAP) + idx as f32 * row_h;
                if idx == self.selected {
                    add_rounded_box(
                        c,
                        &children,
                        px(ROW_INSET_X),
                        row_y,
                        row_w,
                        row_h,
                        px(ROW_RADIUS),
                        highlight_fill,
                        Some(highlight_border),
                    )?;
                }

                let kind_glyph = entry_glyph(&entry.kind);
                let icon = gfx.glyph_brush(&[(kind_glyph, 0.88)], scale, ink)?;
                add_visual(
                    c,
                    &children,
                    &icon.brush.cast()?,
                    px(ROW_INSET_X + 12.0),
                    row_y + (row_h - icon.height) / 2.0,
                    icon.width,
                    icon.height,
                )?;

                let text_x = px(ROW_INSET_X + 42.0);
                let text_max_w = (phys_w - text_x - px(ROW_INSET_X + 14.0)).max(px(80.0));
                let title_lbl =
                    gfx.text_brush_fitted(&entry.title, scale, 14.0, true, ink, text_max_w)?;
                add_visual(
                    c,
                    &children,
                    &title_lbl.brush.cast()?,
                    text_x,
                    row_y + px(7.0),
                    title_lbl.width,
                    title_lbl.height,
                )?;

                let subtitle = entry_subtitle(entry, t);
                let sub_lbl =
                    gfx.text_brush_fitted(&subtitle, scale, 11.5, false, ink, text_max_w)?;
                add_visual(
                    c,
                    &children,
                    &sub_lbl.brush.cast()?,
                    text_x,
                    row_y + px(26.0),
                    sub_lbl.width,
                    sub_lbl.height,
                )?
                .SetOpacity(0.66)?;
            }
        }

        content.SetOffset(Vector3::new(0.0, 0.0, 0.0))?;
        layers.InsertAtTop(&content)?;
        Ok(())
    }

    fn animate_entrance(
        &self,
        gfx: &Gfx,
        phys_w: f32,
        base_x: f32,
        base_y: f32,
        scale: f32,
    ) -> Result<()> {
        let c = &gfx.compositor;
        self.surface.SetOffset(Vector3::new(base_x, base_y, 0.0))?;
        self.surface
            .SetCenterPoint(Vector3::new(phys_w / 2.0, 0.0, 0.0))?;

        let spring_ease =
            c.CreateCubicBezierEasingFunction(Vector2::new(0.22, 1.0), Vector2::new(0.36, 1.0))?;
        let settle_ease =
            c.CreateCubicBezierEasingFunction(Vector2::new(0.3, 0.0), Vector2::new(0.2, 1.0))?;
        let main_duration = TimeSpan {
            Duration: 220 * 10_000,
        };
        let fade_duration = TimeSpan {
            Duration: 150 * 10_000,
        };

        let scale_anim = c.CreateVector3KeyFrameAnimation()?;
        scale_anim.SetDuration(main_duration)?;
        scale_anim.InsertKeyFrame(0.0, Vector3::new(0.94, 0.94, 1.0))?;
        scale_anim.InsertKeyFrameWithEasingFunction(
            0.68,
            Vector3::new(1.012, 1.012, 1.0),
            &spring_ease,
        )?;
        scale_anim.InsertKeyFrameWithEasingFunction(
            1.0,
            Vector3::new(1.0, 1.0, 1.0),
            &settle_ease,
        )?;

        let slide_anim = c.CreateScalarKeyFrameAnimation()?;
        slide_anim.SetDuration(main_duration)?;
        slide_anim.InsertKeyFrame(0.0, base_y - 12.0 * scale)?;
        slide_anim.InsertKeyFrameWithEasingFunction(1.0, base_y, &spring_ease)?;

        let fade_anim = c.CreateScalarKeyFrameAnimation()?;
        fade_anim.SetDuration(fade_duration)?;
        fade_anim.InsertKeyFrame(0.0, 0.0)?;
        fade_anim.InsertKeyFrameWithEasingFunction(1.0, 1.0, &spring_ease)?;

        self.surface.StartAnimation(h!("Scale"), &scale_anim)?;
        self.surface.StartAnimation(h!("Offset.Y"), &slide_anim)?;
        self.surface.StartAnimation(h!("Opacity"), &fade_anim)?;
        Ok(())
    }

    fn reset_surface_transform(&self, base_x: f32, base_y: f32) -> Result<()> {
        self.surface.StopAnimation(h!("Scale"))?;
        self.surface.StopAnimation(h!("Offset.Y"))?;
        self.surface.StopAnimation(h!("Opacity"))?;
        self.surface.SetScale(Vector3::new(1.0, 1.0, 1.0))?;
        self.surface.SetOffset(Vector3::new(base_x, base_y, 0.0))?;
        self.surface.SetOpacity(1.0)?;
        Ok(())
    }
}

fn entry_glyph(kind: &EntryKind) -> char {
    match kind {
        EntryKind::App { .. } => glyph::APP_BADGE,
        EntryKind::Math { .. } => glyph::BOLT,
        EntryKind::Command(_) => glyph::SETTINGS,
    }
}

fn entry_subtitle<'a>(entry: &'a Entry, t: &'static strings::Strings) -> Cow<'a, str> {
    match &entry.kind {
        EntryKind::App { .. } => Cow::Borrowed(t.spotlight_app_hint),
        EntryKind::Math { .. } => {
            if entry.subtitle.is_empty() {
                Cow::Borrowed(t.spotlight_math_hint)
            } else {
                Cow::Owned(format!("{} · {}", entry.subtitle, t.spotlight_math_hint))
            }
        }
        EntryKind::Command(_) => {
            if entry.subtitle.is_empty() {
                Cow::Borrowed(t.spotlight_cmd_hint)
            } else {
                Cow::Owned(format!("{} · {}", entry.subtitle, t.spotlight_cmd_hint))
            }
        }
    }
}

/// Computes the logical height of the Spotlight overlay for `row_count` results.
#[must_use]
fn panel_logical_height(row_count: usize, has_query: bool) -> f32 {
    let visible_rows = row_count.min(MAX_RESULTS);
    if visible_rows > 0 {
        INPUT_HEIGHT + LIST_TOP_GAP + visible_rows as f32 * ROW_HEIGHT + LIST_BOTTOM_PAD
    } else if has_query {
        INPUT_HEIGHT + LIST_TOP_GAP + ROW_HEIGHT + LIST_BOTTOM_PAD
    } else {
        INPUT_HEIGHT
    }
}

/// Moves the selected row index by `delta` (`-1` for Up, `+1` for Down) with wrap-around.
#[must_use]
fn move_selection(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current_clamped = current.min(len - 1);
    let len_isize = len as isize;
    ((current_clamped as isize + delta).rem_euclid(len_isize)) as usize
}

/// Maps a physical client-space `y` coordinate to a result row index, if hit.
#[must_use]
fn hit_test_row(y_physical: f32, scale: f32, row_count: usize) -> Option<usize> {
    let visible_rows = row_count.min(MAX_RESULTS);
    if visible_rows == 0 || scale <= 0.0 || !y_physical.is_finite() {
        return None;
    }
    let y_logical = y_physical / scale;
    let list_top = INPUT_HEIGHT + LIST_TOP_GAP;
    if y_logical < list_top {
        return None;
    }
    let offset = y_logical - list_top;
    let index = (offset / ROW_HEIGHT).floor() as usize;
    (index < visible_rows).then_some(index)
}

fn primary_screen_metrics() -> (i32, i32, i32, i32, f32) {
    if let Some(monitor) = system::primary_monitor() {
        let w = (monitor.screen.right - monitor.screen.left).max(1);
        let h = (monitor.screen.bottom - monitor.screen.top).max(1);
        (
            monitor.screen.left,
            monitor.screen.top,
            w,
            h,
            monitor.scale.max(0.5),
        )
    } else {
        (0, 0, 1920, 1080, 1.0)
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

unsafe extern "system" fn spotlight_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_CHAR => {
            if let Some(ch) = u32::try_from(w.0).ok().and_then(char::from_u32) {
                crate::app::spotlight_char(ch);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            crate::app::spotlight_key_down((w.0 & 0xFFFF) as u16);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (l.0 & 0xFFFF) as i16;
            let y = ((l.0 >> 16) & 0xFFFF) as i16;
            crate::app::spotlight_click(f32::from(x), f32::from(y));
            LRESULT(0)
        }
        WM_ACTIVATE if (w.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            if SPOTLIGHT_OPEN.load(Ordering::Relaxed) {
                crate::app::spotlight_deactivate();
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // SAFETY: Keep the standard arrow cursor over the overlay.
            if let Ok(arrow) = unsafe { LoadCursorW(None, IDC_ARROW) } {
                unsafe { SetCursor(Some(arrow)) };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_navigation_wraps_in_both_directions() {
        assert_eq!(move_selection(0, 0, 1), 0);
        assert_eq!(move_selection(0, 0, -1), 0);
        assert_eq!(move_selection(0, 1, 1), 0);
        assert_eq!(move_selection(0, 1, -1), 0);

        assert_eq!(move_selection(0, 5, 1), 1);
        assert_eq!(move_selection(4, 5, 1), 0);
        assert_eq!(move_selection(0, 5, -1), 4);
        assert_eq!(move_selection(3, 5, -1), 2);
        // Out-of-bounds current index clamps cleanly before moving.
        assert_eq!(move_selection(99, 3, 1), 0);
    }

    #[test]
    fn row_hit_testing_handles_bounds_and_dpi_scaling() {
        assert_eq!(hit_test_row(80.0, 1.0, 0), None);
        assert_eq!(hit_test_row(80.0, 0.0, 3), None);
        assert_eq!(hit_test_row(f32::NAN, 1.0, 3), None);

        // Above the list (inside the 64px input bar and 8px gap).
        assert_eq!(hit_test_row(0.0, 1.0, 4), None);
        assert_eq!(hit_test_row(71.9, 1.0, 4), None);

        // Exact row intervals at 1.0x scale: list starts at 72.0, each row is 48.0.
        assert_eq!(hit_test_row(72.0, 1.0, 4), Some(0));
        assert_eq!(hit_test_row(119.9, 1.0, 4), Some(0));
        assert_eq!(hit_test_row(120.0, 1.0, 4), Some(1));
        assert_eq!(hit_test_row(168.0, 1.0, 4), Some(2));
        assert_eq!(hit_test_row(216.0, 1.0, 4), Some(3));
        assert_eq!(hit_test_row(264.0, 1.0, 4), None);

        // Scaled coordinates at 1.5x DPI.
        assert_eq!(hit_test_row(72.0 * 1.5 - 0.1, 1.5, 3), None);
        assert_eq!(hit_test_row(72.0 * 1.5, 1.5, 3), Some(0));
        assert_eq!(hit_test_row((72.0 + 48.0) * 1.5 + 1.0, 1.5, 3), Some(1));
        assert_eq!(hit_test_row((72.0 + 96.0) * 1.5 + 1.0, 1.5, 3), Some(2));
        assert_eq!(hit_test_row((72.0 + 144.0) * 1.5, 1.5, 3), None);
    }

    #[test]
    fn panel_height_matches_row_limits() {
        assert_eq!(panel_logical_height(0, false), 64.0);
        assert_eq!(panel_logical_height(0, true), 64.0 + 8.0 + 48.0 + 12.0);
        assert_eq!(panel_logical_height(1, true), 64.0 + 8.0 + 48.0 + 12.0);
        assert_eq!(panel_logical_height(7, true), 64.0 + 8.0 + 7.0 * 48.0 + 12.0);
        assert_eq!(panel_logical_height(20, true), 64.0 + 8.0 + 7.0 * 48.0 + 12.0);
    }
}
