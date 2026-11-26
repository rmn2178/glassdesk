//! Hover-only window picker. DWM owns the live previews; the dock draws cards
//! and titles without capturing or retaining the contents of other windows.

use std::time::{Duration, Instant};

use anyhow::Result;
use nosih_core::frame::DockEdge;
use nosih_shell::{system::animations_enabled, tracker};
use windows::{
    UI::{
        Color,
        Composition::{
            CompositionSurfaceBrush, ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual,
        },
    },
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, HWND_TOPMOST,
                IDC_ARROW, KillTimer, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SWP_NOACTIVATE,
                SWP_SHOWWINDOW, SetTimer, SetWindowPos, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP,
                WM_MOUSEACTIVATE, WM_MOUSEWHEEL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_POPUP,
            },
        },
    },
    core::{BOOL, Interface, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    app,
    gfx::{Gfx, Ink},
    scene::Theme,
    strings::text,
    thumbnails::Thumbnails,
};

const TIMER: usize = 1;
const PADDING: f32 = 18.0;
const GAP: f32 = 12.0;
const CARD_WIDTH: f32 = 216.0;
const CARD_HEIGHT: f32 = 164.0;
const HEADER: f32 = 24.0;
const ARC: f32 = 16.0;
const PREVIEW_INSET: f32 = 4.0;
const CLOSE_BUTTON_GAP: f32 = 4.0;
const OPEN_SECONDS: f32 = 0.2;
const LEAVE_DELAY: Duration = Duration::from_millis(220);

/// Pure layout shared by rendering and hit testing, including negative-origin
/// monitors. Overflow is paged instead of shrinking previews to unreadable sizes.
struct MultiWindowLayout {
    bounds: RECT,
    cards: Vec<RECT>,
    per_page: usize,
    /// The screen edge the dock is on: the view opens away from it.
    edge: DockEdge,
}

impl MultiWindowLayout {
    fn new(screen: RECT, anchor: RECT, edge: DockEdge, scale: f32, count: usize) -> Self {
        let gap = (10.0 * scale) as i32;
        // On a side dock the row of previews sits beside the icon, in the
        // width left between the dock and the far side of the screen.
        let room = match edge {
            DockEdge::Bottom => screen.right - screen.left,
            DockEdge::Left => screen.right - anchor.right - gap,
            DockEdge::Right => anchor.left - gap - screen.left,
        };
        let available = (room as f32 - 32.0 * scale).max(1.0);
        let per_page = (((available - 2.0 * PADDING * scale) / ((CARD_WIDTH + GAP) * scale))
            as usize)
            .clamp(1, 5);
        let shown = count.min(per_page);
        let header = if count > per_page { HEADER } else { 0.0 };
        let card_width = (CARD_WIDTH * scale).min((available - 2.0 * PADDING * scale).max(1.0));
        let width = (2.0 * PADDING * scale
            + shown as f32 * card_width
            + shown.saturating_sub(1) as f32 * GAP * scale)
            .ceil() as i32;
        let height = ((2.0 * PADDING + header + CARD_HEIGHT + ARC) * scale).ceil() as i32;
        let middle_y = (anchor.top + (anchor.bottom - anchor.top) / 2) - height / 2;
        let beside_top = middle_y.clamp(screen.top, (screen.bottom - height).max(screen.top));
        let (left, top) = match edge {
            DockEdge::Bottom => (
                ((anchor.left + (anchor.right - anchor.left) / 2) - width / 2)
                    .clamp(screen.left, (screen.right - width).max(screen.left)),
                (anchor.top - height - gap).max(screen.top),
            ),
            DockEdge::Left => ((anchor.right + gap).min(screen.right - width), beside_top),
            DockEdge::Right => ((anchor.left - gap - width).max(screen.left), beside_top),
        };
        let cards = (0..shown)
            .map(|index| {
                let across = if shown > 1 {
                    index as f32 / (shown - 1) as f32 * 2.0 - 1.0
                } else {
                    0.0
                };
                let x = PADDING * scale + index as f32 * (card_width + GAP * scale);
                let y = (PADDING + header + ARC * across * across) * scale;
                RECT {
                    left: x.round() as i32,
                    top: y.round() as i32,
                    right: (x + card_width).round() as i32,
                    bottom: (y + CARD_HEIGHT * scale).round() as i32,
                }
            })
            .collect();
        Self {
            bounds: RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            },
            cards,
            per_page,
            edge,
        }
    }

    fn hit(&self, point: POINT) -> Option<usize> {
        self.cards.iter().position(|rect| contains(*rect, point))
    }

    fn hit_close(&self, point: POINT, scale: f32) -> Option<usize> {
        self.cards.iter().position(|rect| {
            let card_rect = nosih_core::layout::Rect {
                x: rect.left as f32,
                y: rect.top as f32,
                width: (rect.right - rect.left) as f32,
                height: (rect.bottom - rect.top) as f32,
            };
            let bounds = nosih_core::preview::close_button_bounds(card_rect, scale);
            nosih_core::preview::contains(
                bounds.hit,
                nosih_core::preview::Point {
                    x: point.x as f32,
                    y: point.y as f32,
                },
            )
        })
    }

    fn keeps_open(&self, anchor: RECT, point: POINT) -> bool {
        if contains(self.bounds, point) || contains(anchor, point) {
            return true;
        }
        // Widen towards the whole picker: a diagonal path to an outer card
        // must stay connected, even when the picker is clamped to a screen edge.
        // The narrow end still belongs only to the source icon.
        match self.edge {
            DockEdge::Bottom => in_handoff(
                (point.x, point.y),
                (anchor.left, anchor.right),
                (self.bounds.left, self.bounds.right),
                (anchor.top, self.bounds.bottom),
            ),
            DockEdge::Left => in_handoff(
                (point.y, point.x),
                (anchor.top, anchor.bottom),
                (self.bounds.top, self.bounds.bottom),
                (anchor.right, self.bounds.left),
            ),
            DockEdge::Right => in_handoff(
                (point.y, point.x),
                (anchor.top, anchor.bottom),
                (self.bounds.top, self.bounds.bottom),
                (anchor.left, self.bounds.right),
            ),
        }
    }
}

fn in_handoff(
    point: (i32, i32),
    source: (i32, i32),
    picker: (i32, i32),
    faces: (i32, i32),
) -> bool {
    let distance = f64::from(faces.1) - f64::from(faces.0);
    if distance == 0.0 {
        return false;
    }
    let t = (f64::from(point.1) - f64::from(faces.0)) / distance;
    if !(0.0..=1.0).contains(&t) {
        return false;
    }
    let left = f64::from(source.0) + (f64::from(picker.0) - f64::from(source.0)) * t;
    let right = f64::from(source.1) + (f64::from(picker.1) - f64::from(source.1)) * t;
    f64::from(point.0) >= left && f64::from(point.0) < right
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

/// The thumbnail starts under the close button.
fn thumbnail_top_inset(scale: f32) -> f32 {
    let close = nosih_core::preview::close_button_bounds(
        nosih_core::layout::Rect {
            x: 0.0,
            y: 0.0,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
        },
        scale,
    );
    close.visual.y + close.visual.height + CLOSE_BUTTON_GAP * scale.max(0.5)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiWindowAction {
    Activate(isize),
    Close(isize),
}

struct Card {
    root: ContainerVisual,
    highlight: SpriteVisual,
    close_btn: SpriteVisual,
    close_normal_brush: CompositionSurfaceBrush,
    close_hover_brush: CompositionSurfaceBrush,
}

pub struct MultiWindowView {
    pub app_id: String,
    hwnd: HWND,
    target: Option<DesktopWindowTarget>,
    thumbnails: Thumbnails,
    windows: Vec<isize>,
    layout: MultiWindowLayout,
    anchor: RECT,
    scale: f32,
    theme: Theme,
    page: usize,
    cards: Vec<Card>,
    opened: Instant,
    animated: bool,
    left_at: Option<Instant>,
    pressed: Option<usize>,
    close_hovered: Option<usize>,
    close_pressed: Option<usize>,
    mouse_was_down: bool,
    foreground: isize,
    /// The card lit when the opened view was last drawn. Once open, only that
    /// changes what it shows, so a tick that finds it the same touches
    /// nothing: no composition batch and no DWM work while the view rests.
    drawn: Option<Option<usize>>,
}

/// Where the view opens: the app's icon and display, and the screen edge
/// the dock is on.
pub struct Anchor {
    pub icon: RECT,
    pub screen: RECT,
    pub edge: DockEdge,
}

impl MultiWindowView {
    pub fn refresh_backdrops(&self, gfx: &Gfx) -> Result<()> {
        if let Some(target) = &self.target {
            gfx.refresh_backdrops(&target.Root()?.cast()?)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        gfx: &Gfx,
        app_id: String,
        windows: Vec<isize>,
        at: &Anchor,
        scale: f32,
        theme: Theme,
    ) -> Result<Self> {
        // SAFETY: Class and window belong to this UI thread. Once created,
        // `view` owns the HWND even if subsequent graphics setup fails.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("NosihMultiWindowView");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(multi_window_view_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Nosih window previews"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        let mut view = Self {
            app_id,
            hwnd,
            target: None,
            thumbnails: Thumbnails::new(hwnd),
            layout: MultiWindowLayout::new(at.screen, at.icon, at.edge, scale, windows.len()),
            windows,
            anchor: at.icon,
            scale,
            theme,
            page: 0,
            cards: Vec::new(),
            opened: Instant::now(),
            drawn: None,
            animated: animations_enabled(),
            left_at: None,
            pressed: None,
            close_hovered: None,
            close_pressed: None,
            mouse_was_down: false,
            foreground: tracker::foreground_window(),
        };
        // Match the dock's native backdrop setup. Older Windows versions can
        // reject the attribute; their theme already supplies an opaque tint.
        let enable = BOOL::from(true);
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const enable).cast(),
                size_of::<BOOL>() as u32,
            );
        }
        // SAFETY: This process owns the top-level destination window.
        view.target = Some(unsafe {
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        });
        view.rebuild(gfx)?;
        let bounds = view.layout.bounds;
        // SAFETY: Show our window without stealing focus from the active app.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
            SetTimer(Some(hwnd), TIMER, 16, None);
        }
        view.update()?;
        Ok(view)
    }

    pub fn matches(&self, windows: &[isize]) -> bool {
        self.windows.len() == windows.len()
            && self.windows.iter().all(|hwnd| windows.contains(hwnd))
    }

    fn local_cursor(&self) -> Result<POINT> {
        let mut point = POINT::default();
        // SAFETY: Valid output buffer for a read-only query.
        unsafe {
            GetCursorPos(&mut point)?;
        }
        point.x -= self.layout.bounds.left;
        point.y -= self.layout.bounds.top;
        Ok(point)
    }

    fn rebuild(&mut self, gfx: &Gfx) -> Result<()> {
        let c = &gfx.compositor;
        let root = c.CreateContainerVisual()?;
        let layers = root.Children()?;
        let width = (self.layout.bounds.right - self.layout.bounds.left) as f32;
        let height = (self.layout.bounds.bottom - self.layout.bounds.top) as f32;
        let panel = self.theme.panel(
            gfx,
            Vector2 {
                X: width,
                Y: height,
            },
            18.0 * self.scale,
            self.scale,
        )?;
        layers.InsertAtTop(&panel)?;
        let ink = if self.theme.light {
            Ink::Dark
        } else {
            Ink::Light
        };
        if self.windows.len() > self.layout.per_page {
            let heading = format!(
                "{}–{} / {}",
                self.page + 1,
                (self.page + self.layout.per_page).min(self.windows.len()),
                self.windows.len()
            );
            let label = gfx.text_brush(&heading, self.scale, true, ink)?;
            let header = c.CreateSpriteVisual()?;
            header.SetBrush(&label.brush)?;
            header.SetSize(Vector2 {
                X: label.width.min(width - 2.0 * PADDING * self.scale),
                Y: label.height,
            })?;
            header.SetOffset(Vector3 {
                X: PADDING * self.scale,
                Y: 12.0 * self.scale,
                Z: 0.0,
            })?;
            layers.InsertAtTop(&header)?;
        }
        self.cards.clear();
        let shown =
            &self.windows[self.page..(self.page + self.layout.per_page).min(self.windows.len())];
        self.thumbnails.sync(shown);
        for (&hwnd, rect) in shown.iter().zip(&self.layout.cards) {
            let card = c.CreateContainerVisual()?;
            let size = Vector2 {
                X: (rect.right - rect.left) as f32,
                Y: (rect.bottom - rect.top) as f32,
            };
            card.SetSize(size)?;
            let shape = c.CreateRoundedRectangleGeometry()?;
            shape.SetSize(size)?;
            shape.SetCornerRadius(Vector2 {
                X: nosih_core::preview::PREVIEW_CARD_CORNER_RADIUS * self.scale,
                Y: nosih_core::preview::PREVIEW_CARD_CORNER_RADIUS * self.scale,
            })?;
            card.SetClip(&c.CreateGeometricClipWithGeometry(&shape)?)?;
            let background = c.CreateSpriteVisual()?;
            background.SetSize(size)?;
            background.SetBrush(&c.CreateColorBrushWithColor(if self.theme.light {
                Color {
                    A: 56,
                    R: 241,
                    G: 241,
                    B: 244,
                }
            } else {
                Color {
                    A: 40,
                    R: 38,
                    G: 38,
                    B: 43,
                }
            })?)?;
            card.Children()?.InsertAtTop(&background)?;
            let highlight = c.CreateSpriteVisual()?;
            highlight.SetSize(size)?;
            highlight.SetBrush(&c.CreateColorBrushWithColor(Color {
                A: 75,
                R: 80,
                G: 150,
                B: 255,
            })?)?;
            highlight.SetOpacity(0.0)?;
            card.Children()?.InsertAtTop(&highlight)?;
            let title = tracker::title(hwnd);
            let mut title = if title.is_empty() {
                text().new_window.replace('&', "")
            } else {
                title
            };
            if title.chars().count() > 27 {
                title = title.chars().take(26).chain(['…']).collect();
            }
            let label = gfx.text_brush(&title, self.scale, false, ink)?;
            let caption = c.CreateSpriteVisual()?;
            caption.SetBrush(&label.brush)?;
            caption.SetSize(Vector2 {
                X: label.width,
                Y: label.height,
            })?;
            caption.SetOffset(Vector3 {
                X: 10.0 * self.scale,
                Y: size.Y - 29.0 * self.scale,
                Z: 0.0,
            })?;
            card.Children()?.InsertAtTop(&caption)?;

            let (close_btn, close_normal_brush, close_hover_brush) = {
                let card_rect = nosih_core::layout::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.X,
                    height: size.Y,
                };
                let btn_bounds = nosih_core::preview::close_button_bounds(card_rect, self.scale);
                let btn_side = btn_bounds.visual.width as u32;
                let normal_brush = gfx.close_button_brush(btn_side, false, self.theme.light)?;
                let hover_brush = gfx.close_button_brush(btn_side, true, self.theme.light)?;

                let btn_visual = c.CreateSpriteVisual()?;
                btn_visual.SetBrush(&normal_brush)?;
                btn_visual.SetSize(Vector2 {
                    X: btn_bounds.visual.width,
                    Y: btn_bounds.visual.height,
                })?;
                btn_visual.SetOffset(Vector3 {
                    X: btn_bounds.visual.x,
                    Y: btn_bounds.visual.y,
                    Z: 0.0,
                })?;
                btn_visual.SetOpacity(0.0)?;
                card.Children()?.InsertAtTop(&btn_visual)?;

                (btn_visual, normal_brush, hover_brush)
            };

            layers.InsertAtTop(&card)?;
            self.cards.push(Card {
                root: card,
                highlight,
                close_btn,
                close_normal_brush,
                close_hover_brush,
            });
        }
        if let Some(target) = &self.target {
            target.SetRoot(&root)?;
        }
        self.opened = Instant::now();
        self.drawn = None;
        Ok(())
    }

    fn update(&mut self) -> Result<()> {
        let elapsed = self.opened.elapsed().as_secs_f32();
        let progress = if self.animated {
            (elapsed / OPEN_SECONDS).min(1.0)
        } else {
            1.0
        };
        let eased = 1.0 - (1.0 - progress).powi(3);
        let cursor = self.local_cursor()?;
        let hovered = self.layout.hit(cursor);
        let close_hovered = self.layout.hit_close(cursor, self.scale);
        if progress >= 1.0 && self.drawn == Some(hovered) && self.close_hovered == close_hovered {
            return Ok(());
        }
        self.close_hovered = close_hovered;
        let center = (self.anchor.left + (self.anchor.right - self.anchor.left) / 2
            - self.layout.bounds.left) as f32;
        let travel = (1.0 - eased) * 36.0 * self.scale;
        for (index, (card, rect)) in self.cards.iter().zip(&self.layout.cards).enumerate() {
            let width = (rect.right - rect.left) as f32;
            // Above a bottom dock the cards fan out of the icon and rise; beside
            // a side dock they slide out of it.
            let (x, y) = match self.layout.edge {
                DockEdge::Bottom => {
                    let start_x = center - width / 2.0;
                    (
                        start_x + (rect.left as f32 - start_x) * eased,
                        rect.top as f32 + travel,
                    )
                }
                DockEdge::Left => (rect.left as f32 - travel, rect.top as f32),
                DockEdge::Right => (rect.left as f32 + travel, rect.top as f32),
            };
            card.root.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            card.root.SetOpacity(eased)?;
            card.highlight
                .SetOpacity(if hovered == Some(index) { 1.0 } else { 0.0 })?;
            card.close_btn.SetBrush(if close_hovered == Some(index) {
                &card.close_hover_brush
            } else {
                &card.close_normal_brush
            })?;
            card.close_btn
                .SetOpacity(if hovered == Some(index) { 1.0 } else { 0.0 })?;
            if let Some(&hwnd) = self.windows.get(self.page + index) {
                self.thumbnails.place_preview(
                    hwnd,
                    RECT {
                        left: (x + PREVIEW_INSET * self.scale).round() as i32,
                        top: (y + thumbnail_top_inset(self.scale)).round() as i32,
                        right: (x + width - PREVIEW_INSET * self.scale).round() as i32,
                        bottom: (y + (CARD_HEIGHT - 31.0) * self.scale).round() as i32,
                    },
                    (255.0 * eased) as u8,
                );
            }
        }
        self.drawn = (progress >= 1.0).then_some(hovered);
        // Run quickly only during the opening view animation. Nothing is polled
        // when the popup is closed, and thumbnail pixels are maintained by DWM.
        unsafe {
            SetTimer(
                Some(self.hwnd),
                TIMER,
                if progress < 1.0 { 16 } else { 60 },
                None,
            );
        }
        Ok(())
    }

    /// Returns false after Escape, an outside click, or leaving the view/icon bridge.
    pub fn tick(&mut self) -> Result<bool> {
        let foreground = tracker::foreground_window();
        let mut point = POINT::default();
        // SAFETY: Read-only pointer/key state queries; no global input hook.
        let (escape, mouse_down) = unsafe {
            GetCursorPos(&mut point)?;
            (
                GetAsyncKeyState(i32::from(VK_ESCAPE.0)) < 0,
                GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0,
            )
        };
        self.tick_at(point, foreground, escape, mouse_down, Instant::now())
    }

    fn tick_at(
        &mut self,
        point: POINT,
        foreground: isize,
        escape: bool,
        mouse_down: bool,
        now: Instant,
    ) -> Result<bool> {
        if foreground != self.foreground {
            return Ok(false);
        }
        let inside = self.layout.keeps_open(self.anchor, point);
        if escape || (mouse_down && !self.mouse_was_down && !inside) {
            return Ok(false);
        }
        self.mouse_was_down = mouse_down;
        if inside {
            self.left_at = None;
        } else if now.saturating_duration_since(*self.left_at.get_or_insert(now)) >= LEAVE_DELAY {
            return Ok(false);
        }
        self.update()?;
        Ok(true)
    }

    pub fn press(&mut self) -> Result<()> {
        self.press_at(self.local_cursor()?);
        Ok(())
    }

    fn press_at(&mut self, point: POINT) {
        // Do not select a card while it is still moving past another card.
        if !self.animated || self.opened.elapsed().as_secs_f32() >= OPEN_SECONDS {
            if let Some(index) = self.layout.hit_close(point, self.scale) {
                self.close_pressed = Some(index);
                self.pressed = None;
                return;
            }
            self.close_pressed = None;
            self.pressed = self.layout.hit(point);
        } else {
            self.close_pressed = None;
            self.pressed = None;
        }
    }

    pub fn release(&mut self) -> Result<Option<MultiWindowAction>> {
        Ok(self.release_at(self.local_cursor()?))
    }

    fn release_at(&mut self, point: POINT) -> Option<MultiWindowAction> {
        if let Some(pressed_close) = self.close_pressed.take()
            && self.layout.hit_close(point, self.scale) == Some(pressed_close)
        {
            return self
                .windows
                .get(self.page + pressed_close)
                .copied()
                .map(MultiWindowAction::Close);
        }
        let hit = self.layout.hit(point);
        self.pressed
            .take()
            .filter(|pressed| Some(*pressed) == hit)
            .and_then(|index| self.windows.get(self.page + index).copied())
            .map(MultiWindowAction::Activate)
    }

    pub fn middle_click(&mut self) -> Result<Option<isize>> {
        let point = self.local_cursor()?;
        let hit = self.layout.hit(point);
        Ok(hit.and_then(|index| self.windows.get(self.page + index).copied()))
    }

    pub fn sync_windows(&mut self, gfx: &Gfx, windows: Vec<isize>) -> Result<bool> {
        self.windows = windows;
        if self.windows.is_empty() {
            return Ok(false);
        }
        if self.page >= self.windows.len() {
            self.page = self.page.saturating_sub(self.layout.per_page);
        }
        let screen = RECT {
            left: self.layout.bounds.left,
            top: self.layout.bounds.top,
            right: self.layout.bounds.right,
            bottom: self.layout.bounds.bottom,
        };
        self.layout = MultiWindowLayout::new(
            screen,
            self.anchor,
            self.layout.edge,
            self.scale,
            self.windows.len(),
        );
        let bounds = self.layout.bounds;
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        self.rebuild(gfx)?;
        self.update()?;
        Ok(true)
    }

    pub fn remove_window(&mut self, gfx: &Gfx, hwnd: isize) -> Result<bool> {
        let mut windows = self.windows.clone();
        windows.retain(|&h| h != hwnd);
        self.sync_windows(gfx, windows)
    }

    pub fn scroll(&mut self, gfx: &Gfx, previous: bool) -> Result<()> {
        let page = if previous {
            self.page.saturating_sub(self.layout.per_page)
        } else {
            (self.page + self.layout.per_page).min(
                self.windows.len().saturating_sub(1) / self.layout.per_page * self.layout.per_page,
            )
        };
        if page != self.page {
            self.page = page;
            self.pressed = None;
            self.rebuild(gfx)?;
            self.update()?;
        }
        Ok(())
    }
}

impl Drop for MultiWindowView {
    fn drop(&mut self) {
        // Release DWM relationships and composition target before the HWND.
        self.thumbnails.sync(&[]);
        self.target = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn multi_window_view_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if w.0 == TIMER => app::multi_window_view_tick(),
        WM_LBUTTONDOWN => app::multi_window_view_press(),
        WM_LBUTTONUP => app::multi_window_view_release(),
        WM_MBUTTONUP => app::multi_window_view_middle_click(),
        WM_RBUTTONUP => app::multi_window_view_dismiss(),
        WM_MOUSEWHEEL => app::multi_window_view_scroll(((w.0 >> 16) as i16) > 0),
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_starts_below_close_button_when_enabled() {
        for scale in [0.75, 1.0, 1.1, 1.25, 1.5, 2.0] {
            let close = nosih_core::preview::close_button_bounds(
                nosih_core::layout::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: CARD_WIDTH,
                    height: CARD_HEIGHT,
                },
                scale,
            );
            assert!(
                thumbnail_top_inset(scale)
                    >= close.visual.y + close.visual.height + CLOSE_BUTTON_GAP * scale
            );
        }
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop and DWM"]
    fn native_multi_window_view_should_page_select_without_focus_and_release_its_window()
    -> Result<()> {
        use windows::Win32::{
            System::WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
            UI::WindowsAndMessaging::{
                GetForegroundWindow, IsWindow, IsWindowVisible, SW_SHOWNOACTIVATE, ShowWindow,
                WS_OVERLAPPEDWINDOW,
            },
        };
        struct Source(HWND);
        impl Drop for Source {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        // These blank windows belong to the test; no existing user window is
        // activated, moved, closed or used as a preview source.
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let foreground = unsafe { GetForegroundWindow() };
        let mut sources = Vec::new();
        for _ in 0..6 {
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("STATIC"),
                    w!("Nosih preview test"),
                    WS_OVERLAPPEDWINDOW,
                    10,
                    10,
                    300,
                    180,
                    None,
                    None,
                    None,
                    None,
                )?
            };
            let source = Source(hwnd);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            sources.push(source);
        }
        let windows: Vec<_> = sources.iter().map(|source| source.0.0 as isize).collect();
        let gfx = Gfx::new()?;
        let themes = crate::appearance::Material::ALL
            .into_iter()
            .flat_map(|material| {
                [(false, false), (false, true), (true, false), (true, true)].map(
                    |(light, glass)| {
                        Theme::new(crate::appearance::Look {
                            material,
                            light,
                            glass,
                        })
                    },
                )
            });
        for theme in themes {
            let mut view = MultiWindowView::show(
                &gfx,
                "test".into(),
                windows.clone(),
                &Anchor {
                    icon: RECT {
                        left: 280,
                        top: 600,
                        right: 340,
                        bottom: 670,
                    },
                    screen: RECT {
                        left: 0,
                        top: 0,
                        right: 640,
                        bottom: 720,
                    },
                    edge: DockEdge::Bottom,
                },
                1.0,
                theme,
            )?;
            view.animated = false;
            view.update()?;
            let bounds = view.layout.bounds;
            let source = view.anchor;
            let diagonal = POINT {
                x: ((source.left + source.right) / 2 + bounds.left + 18) / 2,
                y: (source.top + bounds.bottom) / 2,
            };
            assert!(
                diagonal.x < source.left,
                "exercise a diagonal outside the old icon-width bridge"
            );
            let now = Instant::now();
            assert!(view.tick_at(diagonal, view.foreground, false, false, now)?);
            assert!(
                view.tick_at(
                    diagonal,
                    view.foreground,
                    false,
                    false,
                    now + LEAVE_DELAY + Duration::from_secs(1)
                )?,
                "moving slowly to another card must not close the picker"
            );
            let outside = POINT {
                x: bounds.right + 100,
                y: bounds.bottom + 100,
            };
            let later = now + Duration::from_secs(2);
            assert!(view.tick_at(outside, view.foreground, false, false, later)?);
            assert!(!view.tick_at(outside, view.foreground, false, false, later + LEAVE_DELAY)?);
            assert!(view.tick_at(diagonal, view.foreground, false, false, later + LEAVE_DELAY)?);
            assert!(!view.tick_at(diagonal, view.foreground, true, false, later + LEAVE_DELAY)?);
            assert!(!view.tick_at(outside, view.foreground, false, true, later + LEAVE_DELAY)?);
            for (&source, rect) in windows.iter().zip(&view.layout.cards) {
                let image = view
                    .thumbnails
                    .placed_bounds(source)
                    .ok_or_else(|| anyhow::anyhow!("DWM did not place the preview"))?;
                assert_eq!(
                    image.bottom,
                    (rect.top as f32 + (CARD_HEIGHT - 31.0) * view.scale).round() as i32,
                    "each preview must align with its card's caption baseline"
                );
                assert!(
                    image.top >= (rect.top as f32 + thumbnail_top_inset(view.scale)).round() as i32,
                    "the DWM thumbnail must leave room for the close button"
                );
            }
            let hwnd = view.hwnd;
            assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
            assert_eq!(
                unsafe { GetForegroundWindow() },
                foreground,
                "hover must not steal focus"
            );
            view.scroll(&gfx, false)?;
            let rect = view.layout.cards[0];
            let point = POINT {
                x: (rect.left + rect.right) / 2,
                y: (rect.top + rect.bottom) / 2,
            };
            view.press_at(point);
            assert_eq!(
                view.release_at(point),
                Some(MultiWindowAction::Activate(windows[view.layout.per_page]))
            );
            view.press_at(point);
            assert_eq!(
                view.release_at(POINT { x: 0, y: 0 }),
                None,
                "dragging off a card must cancel"
            );
            view.scroll(&gfx, true)?;
            view.press_at(point);
            assert_eq!(
                view.release_at(point),
                Some(MultiWindowAction::Activate(windows[0]))
            );
            drop(view);
            assert!(
                !unsafe { IsWindow(Some(hwnd)) }.as_bool(),
                "popup must release its HWND"
            );
        }
        Ok(())
    }

    #[test]
    fn multi_window_view_should_stay_on_screen_at_both_edges_and_high_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            for left in [-1920, -960, -60] {
                let screen = RECT {
                    left: -1920,
                    top: -200,
                    right: 0,
                    bottom: 1080,
                };
                let anchor = RECT {
                    left,
                    top: 950,
                    right: left + 60,
                    bottom: 1020,
                };
                let layout = MultiWindowLayout::new(screen, anchor, DockEdge::Bottom, scale, 12);
                assert!(layout.bounds.left >= screen.left && layout.bounds.right <= screen.right);
                for rect in &layout.cards {
                    assert!(
                        rect.left >= 0 && rect.right <= layout.bounds.right - layout.bounds.left
                    );
                    assert!(
                        rect.top >= 0 && rect.bottom <= layout.bounds.bottom - layout.bounds.top
                    );
                }
            }
        }
    }

    #[test]
    fn beside_a_side_dock_the_previews_open_inward_and_keep_a_bridge() {
        let screen = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for (edge, anchor) in [
            (
                DockEdge::Left,
                RECT {
                    left: 14,
                    top: 480,
                    right: 74,
                    bottom: 540,
                },
            ),
            (
                DockEdge::Right,
                RECT {
                    left: 1846,
                    top: 480,
                    right: 1906,
                    bottom: 540,
                },
            ),
        ] {
            let layout = MultiWindowLayout::new(screen, anchor, edge, 1.25, 7);
            let bounds = layout.bounds;
            assert!(bounds.left >= screen.left && bounds.right <= screen.right);
            assert!(bounds.top >= screen.top && bounds.bottom <= screen.bottom);
            let gap_x = if edge == DockEdge::Left {
                assert!(bounds.left >= anchor.right);
                (anchor.right + bounds.left) / 2
            } else {
                assert!(bounds.right <= anchor.left);
                (bounds.right + anchor.left) / 2
            };
            assert!(layout.keeps_open(anchor, POINT { x: gap_x, y: 510 }));
            assert!(!layout.keeps_open(anchor, POINT { x: gap_x, y: 40 }));
        }
    }

    #[test]
    fn diagonal_paths_reach_each_preview_without_leaving_the_handoff_area() {
        for scale in [1.0, 1.25, 2.0] {
            for origin in [0, -1920] {
                let screen = RECT {
                    left: origin,
                    top: -200,
                    right: origin + 1920,
                    bottom: 1080,
                };
                for (edge, anchor) in [
                    (
                        DockEdge::Bottom,
                        RECT {
                            left: origin + 160,
                            top: 950,
                            right: origin + 240,
                            bottom: 1030,
                        },
                    ),
                    (
                        DockEdge::Left,
                        RECT {
                            left: origin + 14,
                            top: 480,
                            right: origin + 94,
                            bottom: 560,
                        },
                    ),
                    (
                        DockEdge::Right,
                        RECT {
                            left: origin + 1826,
                            top: 480,
                            right: origin + 1906,
                            bottom: 560,
                        },
                    ),
                ] {
                    let layout = MultiWindowLayout::new(screen, anchor, edge, scale, 3);
                    let b = layout.bounds;
                    for fraction in [0.1, 0.5, 0.9] {
                        let (source, target) = match edge {
                            DockEdge::Bottom => (
                                POINT {
                                    x: (anchor.left + anchor.right) / 2,
                                    y: anchor.top,
                                },
                                POINT {
                                    x: b.left + ((b.right - b.left) as f32 * fraction) as i32,
                                    y: b.bottom - 1,
                                },
                            ),
                            DockEdge::Left => (
                                POINT {
                                    x: anchor.right - 1,
                                    y: (anchor.top + anchor.bottom) / 2,
                                },
                                POINT {
                                    x: b.left + 1,
                                    y: b.top + ((b.bottom - b.top) as f32 * fraction) as i32,
                                },
                            ),
                            DockEdge::Right => (
                                POINT {
                                    x: anchor.left,
                                    y: (anchor.top + anchor.bottom) / 2,
                                },
                                POINT {
                                    x: b.right - 1,
                                    y: b.top + ((b.bottom - b.top) as f32 * fraction) as i32,
                                },
                            ),
                        };
                        for step in 0..=10 {
                            let t = step as f32 / 10.0;
                            let point = POINT {
                                x: (source.x as f32 + (target.x - source.x) as f32 * t).round()
                                    as i32,
                                y: (source.y as f32 + (target.y - source.y) as f32 * t).round()
                                    as i32,
                            };
                            assert!(
                                layout.keeps_open(anchor, point),
                                "edge={edge:?} scale={scale} target={fraction} step={step} point={point:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn pointer_should_cross_gap_from_icon_without_dismissing() {
        let anchor = RECT {
            left: 900,
            top: 950,
            right: 960,
            bottom: 1020,
        };
        let layout = MultiWindowLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            anchor,
            DockEdge::Bottom,
            1.0,
            2,
        );
        assert!(layout.keeps_open(
            anchor,
            POINT {
                x: 930,
                y: anchor.top - 5
            }
        ));
        assert!(!layout.keeps_open(
            anchor,
            POINT {
                x: 20,
                y: anchor.top - 5
            }
        ));
    }

    #[test]
    fn every_visible_card_should_have_a_distinct_click_target() {
        let layout = MultiWindowLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 1366,
                bottom: 768,
            },
            RECT {
                left: 600,
                top: 660,
                right: 660,
                bottom: 730,
            },
            DockEdge::Bottom,
            1.0,
            20,
        );
        for (index, rect) in layout.cards.iter().enumerate() {
            assert_eq!(
                layout.hit(POINT {
                    x: (rect.left + rect.right) / 2,
                    y: (rect.top + rect.bottom) / 2
                }),
                Some(index)
            );
        }
        assert!(layout.per_page < 20);
    }
}
