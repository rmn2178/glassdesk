//! A dock group unfolded above its icon when it is clicked, as macOS fans
//! out a Dock stack: a fan of icons with their names going up from the
//! group or, when they do not fit, a grid on a panel. Clicking an app opens
//! it as its own dock icon would; dragging one away takes it out of the
//! group, and the dock carries on with the drag (see `App::start_group_drag`).

use std::time::Instant;

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
        System::{
            LibraryLoader::GetModuleHandleW,
            Ole::{IDropTarget, RegisterDragDrop, RevokeDragDrop},
            WinRT::Composition::ICompositorDesktopInterop,
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, GetCapture, ReleaseCapture, SetCapture, VK_ESCAPE, VK_LBUTTON,
                VK_MBUTTON, VK_RBUTTON,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics,
                HWND_TOPMOST, IDC_ARROW, KillTimer, LoadCursorW, MA_NOACTIVATE, PostMessageW,
                RegisterClassW, SM_CXDRAG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer, SetWindowPos,
                WM_APP, WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
                WM_MOUSEMOVE, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
                WS_POPUP, WindowFromPoint,
            },
        },
    },
    core::{BOOL, Interface, w},
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    app,
    gfx::{Gfx, Ink},
    scene::{AttentionBadge, Theme},
};

const TIMER: usize = 1;
/// Posted to itself when the window loses the mouse it held for a press.
const PRESS_LOST: u32 = WM_APP + 1;
// Dimensions are in logical pixels and are multiplied by the display scale.
/// Room between the group's icon and what unfolds above it.
const LIFT: f32 = 12.0;
const MARGIN: f32 = 12.0;
/// Fan: room between two apps, widest name, and room between name and icon.
const FAN_SPACING: f32 = 14.0;
const FAN_LABEL_MAX: f32 = 220.0;
const FAN_LABEL_GAP: f32 = 10.0;
/// How far right the top of the fan bends, in icons.
const FAN_CURVE: f32 = 0.6;
/// More apps than this unfold as a grid, as a crowded macOS stack does.
const FAN_MAX: usize = 10;
const GRID_PADDING: f32 = 16.0;
const GRID_CELL_MIN: f32 = 92.0;
const GRID_ICON_TOP: f32 = 8.0;
const GRID_TEXT: f32 = 20.0;
const GRID_FONT: f32 = 12.0;
/// Room right of an icon for the dot of an open app.
const DOT_ROOM: f32 = 10.0;
const DOT_SIZE: f32 = 5.0;
/// The hover highlight reaches this far around an icon.
const GLOW: f32 = 6.0;
const OPEN_SECONDS: f32 = 0.22;
/// Each app sets off a little after the one below it.
const STAGGER_SECONDS: f32 = 0.025;

/// One app of the group, as the dock knows it.
pub struct GroupItem {
    pub id: String,
    pub name: String,
    pub icon: Option<CompositionSurfaceBrush>,
    pub running: bool,
}

/// Where and how a group unfolds.
pub struct Placement {
    /// The group's icon on screen.
    pub anchor: RECT,
    pub screen: RECT,
    /// The screen edge the dock is on: the group opens away from it.
    pub edge: DockEdge,
    pub scale: f32,
    /// Side of the apps' icons, in pixels.
    pub icon: f32,
    pub theme: Theme,
    /// The dock's input window: a press there is the dock's business.
    pub dock: HWND,
}

/// What releasing the mouse on the view means.
pub enum Released {
    /// The app pressed and released on.
    Item(String),
    /// A click on the view's empty space: it closes.
    Dismiss,
    Nothing,
}

/// Pure layout, shared by drawing and hit testing. Rectangles are in the
/// window's coordinates; `bounds` is the window on screen.
struct GroupLayout {
    bounds: RECT,
    fan: bool,
    icons: Vec<RECT>,
    /// Grid only: each app's cell, its icon above its name.
    cells: Vec<RECT>,
}

impl GroupLayout {
    fn new(
        screen: RECT,
        anchor: RECT,
        edge: DockEdge,
        scale: f32,
        icon: f32,
        count: usize,
    ) -> Self {
        if edge.is_vertical() {
            return Self::beside(screen, anchor, edge, scale, icon, count);
        }
        let center = (anchor.left + anchor.right) as f32 / 2.0;
        let bottom = anchor.top as f32 - LIFT * scale;
        let pitch = icon + FAN_SPACING * scale;
        let room = bottom - screen.top as f32 - MARGIN * scale - icon;
        let fits = ((room / pitch).floor().max(-1.0) + 1.0) as usize;
        if count <= FAN_MAX.min(fits) {
            Self::fan(screen, center, bottom, scale, icon, count)
        } else {
            Self::grid(screen, center, bottom, scale, icon, count)
        }
    }

    /// The first app sits right above the group; the others go up, the
    /// column bending right, with their names on the left.
    fn fan(screen: RECT, center: f32, bottom: f32, scale: f32, icon: f32, count: usize) -> Self {
        let pitch = icon + FAN_SPACING * scale;
        let curve = FAN_CURVE * icon;
        let margin = MARGIN * scale;
        let left = center - icon / 2.0 - (FAN_LABEL_GAP + FAN_LABEL_MAX) * scale - margin;
        let right = center + curve + icon / 2.0 + DOT_ROOM * scale + margin;
        let top = bottom - count.saturating_sub(1) as f32 * pitch - icon - margin;
        let width = (right - left).ceil();
        // Near a screen edge the whole fan moves over, names and icons alike.
        let fitted = left.round().clamp(
            screen.left as f32,
            (screen.right as f32 - width).max(screen.left as f32),
        );
        let center = center + (fitted - left);
        let left = fitted;
        let top = top.round().max(screen.top as f32);
        let bounds = RECT {
            left: left as i32,
            top: top as i32,
            right: (left + width) as i32,
            bottom: bottom.ceil() as i32,
        };
        let icons = (0..count)
            .map(|index| {
                let along = if count > 1 {
                    index as f32 / (count - 1) as f32
                } else {
                    0.0
                };
                let x = center + curve * along * along - icon / 2.0 - left;
                let y = bottom - index as f32 * pitch - icon - top;
                rect(x, y, icon, icon)
            })
            .collect();
        Self {
            bounds,
            fan: true,
            icons,
            cells: Vec::new(),
        }
    }

    /// Rows of cells on a panel centered over the group. Apps that would
    /// not fit on the screen are left out (it takes a hundred or so).
    fn grid(screen: RECT, center: f32, bottom: f32, scale: f32, icon: f32, count: usize) -> Self {
        let usable = (
            (screen.right - screen.left) as f32 - 2.0 * MARGIN * scale,
            bottom - screen.top as f32 - MARGIN * scale,
        );
        Self::grid_in(scale, icon, count, usable, |width, height| {
            let left = (center - width / 2.0).round().clamp(
                screen.left as f32,
                (screen.right as f32 - width).max(screen.left as f32),
            );
            (left, (bottom - height).round().max(screen.top as f32))
        })
    }

    /// On a side dock, the same grid beside the group, centred on it and
    /// away from the screen edge (a fan is a column: it has no side).
    fn beside(
        screen: RECT,
        anchor: RECT,
        edge: DockEdge,
        scale: f32,
        icon: f32,
        count: usize,
    ) -> Self {
        let gap = LIFT * scale;
        let margin = MARGIN * scale;
        let middle = (anchor.top + anchor.bottom) as f32 / 2.0;
        let (start, end) = if edge == DockEdge::Left {
            (anchor.right as f32 + gap, screen.right as f32 - margin)
        } else {
            (screen.left as f32 + margin, anchor.left as f32 - gap)
        };
        let usable = (
            end - start,
            (screen.bottom - screen.top) as f32 - 2.0 * margin,
        );
        Self::grid_in(scale, icon, count, usable, |width, height| {
            let left = if edge == DockEdge::Left {
                start
            } else {
                end - width
            };
            let top = (middle - height / 2.0).round().clamp(
                screen.top as f32,
                (screen.bottom as f32 - height).max(screen.top as f32),
            );
            (left.round(), top)
        })
    }

    /// The grid's cells for `count` apps within `usable` (width, height),
    /// the panel placed where `place` (its size) says.
    fn grid_in(
        scale: f32,
        icon: f32,
        count: usize,
        usable: (f32, f32),
        place: impl Fn(f32, f32) -> (f32, f32),
    ) -> Self {
        let pad = GRID_PADDING * scale;
        let cell_width = (icon * 1.8).max(GRID_CELL_MIN * scale).round();
        let cell_height = (icon + (2.0 * GRID_ICON_TOP + GRID_TEXT) * scale).round();
        let (usable_width, usable_height) = usable;
        let max_columns = (((usable_width - 2.0 * pad) / cell_width).floor() as usize).max(1);
        let max_rows = (((usable_height - 2.0 * pad) / cell_height).floor() as usize).max(1);
        let columns = ((count as f32).sqrt().ceil() as usize)
            .max(count.div_ceil(max_rows))
            .clamp(1, max_columns);
        let shown = count.min(columns * max_rows);
        let rows = shown.div_ceil(columns).max(1);
        let width = 2.0 * pad + columns as f32 * cell_width;
        let height = 2.0 * pad + rows as f32 * cell_height;
        let (left, top) = place(width, height);
        let bounds = RECT {
            left: left as i32,
            top: top as i32,
            right: (left + width).round() as i32,
            bottom: (top + height).round() as i32,
        };
        let cells: Vec<RECT> = (0..shown)
            .map(|index| {
                rect(
                    pad + (index % columns) as f32 * cell_width,
                    pad + (index / columns) as f32 * cell_height,
                    cell_width,
                    cell_height,
                )
            })
            .collect();
        let icons = cells
            .iter()
            .map(|cell| {
                rect(
                    cell.left as f32 + (cell_width - icon) / 2.0,
                    cell.top as f32 + GRID_ICON_TOP * scale,
                    icon,
                    icon,
                )
            })
            .collect();
        Self {
            bounds,
            fan: false,
            icons,
            cells,
        }
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> RECT {
    RECT {
        left: x.round() as i32,
        top: y.round() as i32,
        right: (x + width).round() as i32,
        bottom: (y + height).round() as i32,
    }
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

struct ItemVisual {
    root: ContainerVisual,
    highlight: SpriteVisual,
    attention: Option<AttentionBadge>,
}

pub struct GroupView {
    pub group_id: String,
    hwnd: HWND,
    dock: HWND,
    target: Option<DesktopWindowTarget>,
    layout: GroupLayout,
    /// The group's apps, in its order, when it opened.
    ids: Vec<String>,
    visuals: Vec<ItemVisual>,
    /// Where each app answers the pointer: its icon and its name.
    targets: Vec<RECT>,
    panel: Option<ContainerVisual>,
    anchor: RECT,
    edge: DockEdge,
    scale: f32,
    theme: Theme,
    opened: Instant,
    animated: bool,
    /// The app pressed, and where the pointer was on screen.
    pressed: Option<(usize, POINT)>,
    mouse_was_down: bool,
    foreground: isize,
    /// The app lit when the unfolded view was last drawn. Once unfolded, only
    /// that changes what it shows, so a tick that finds it the same touches
    /// nothing: no composition batch and no DWM work while the view rests.
    drawn: Option<Option<usize>>,
}

impl GroupView {
    pub fn refresh_backdrops(&self, gfx: &Gfx) -> Result<()> {
        if let Some(target) = &self.target {
            gfx.refresh_backdrops(&target.Root()?.cast()?)?;
        }
        Ok(())
    }

    pub fn show(
        gfx: &Gfx,
        group_id: String,
        items: &[GroupItem],
        place: &Placement,
    ) -> Result<Self> {
        let layout = GroupLayout::new(
            place.screen,
            place.anchor,
            place.edge,
            place.scale,
            place.icon,
            items.len(),
        );
        // SAFETY: Class and window belong to this UI thread. Once created,
        // `view` owns the HWND even if the graphics setup below fails.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("NosihGroupView");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(group_view_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class,
                ..Default::default()
            });
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Nosih group"),
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
        let target: IDropTarget = crate::drop::DropTarget.into();
        // SAFETY: Registering drop target on this window while it lives.
        unsafe {
            let _ = RegisterDragDrop(hwnd, &target);
        }
        let mut view = Self {
            group_id,
            hwnd,
            dock: place.dock,
            target: None,
            layout,
            ids: items.iter().map(|item| item.id.clone()).collect(),
            visuals: Vec::new(),
            targets: Vec::new(),
            panel: None,
            anchor: place.anchor,
            edge: place.edge,
            scale: place.scale,
            theme: place.theme,
            opened: Instant::now(),
            drawn: None,
            animated: animations_enabled(),
            pressed: None,
            mouse_was_down: false,
            foreground: tracker::foreground_window(),
        };
        // The grid's panel is glass, as the dock: same backdrop setup. Older
        // Windows versions reject it; their theme is opaque anyway.
        let enable = BOOL::from(true);
        // SAFETY: Our own window and a live value of the declared size.
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
        view.build(gfx, items, place.theme)?;
        let bounds = view.layout.bounds;
        // SAFETY: Shows our window without taking the focus from the app in front.
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

    /// The group's apps as they were when it opened.
    pub fn apps(&self) -> &[String] {
        &self.ids
    }

    /// Whether a screen point lies inside the group panel.
    pub fn contains(&self, point: POINT) -> bool {
        contains(self.layout.bounds, point)
    }

    pub fn set_attention(&mut self, gfx: &Gfx, pending: impl Fn(&str) -> bool) -> Result<()> {
        for ((id, item), rect) in self
            .ids
            .iter()
            .zip(&mut self.visuals)
            .zip(&self.layout.icons)
        {
            let on = pending(id);
            if on && item.attention.is_none() {
                let badge = AttentionBadge::new(gfx, self.theme)?;
                item.root.Children()?.InsertAtTop(&badge.visual)?;
                item.attention = Some(badge);
            }
            if let Some(badge) = &mut item.attention {
                badge.set_visible(on)?;
                badge.place(
                    nosih_core::layout::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: (rect.right - rect.left) as f32,
                        height: (rect.bottom - rect.top) as f32,
                    },
                    self.scale,
                    1.0,
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn build(&mut self, gfx: &Gfx, items: &[GroupItem], theme: Theme) -> Result<()> {
        let c = &gfx.compositor;
        let s = self.scale;
        let root = c.CreateContainerVisual()?;
        let layers = root.Children()?;
        if !self.layout.fan {
            let bounds = self.layout.bounds;
            let size = v2(
                (bounds.right - bounds.left) as f32,
                (bounds.bottom - bounds.top) as f32,
            );
            let panel = theme.panel(gfx, size, 18.0 * s, s)?;
            layers.InsertAtTop(&panel)?;
            self.panel = Some(panel);
        }
        let ink = if theme.light { Ink::Dark } else { Ink::Light };
        let dot_side = (DOT_SIZE * s).round();
        let dot = c.CreateEllipseGeometry()?;
        dot.SetCenter(v2(dot_side / 2.0, dot_side / 2.0))?;
        dot.SetRadius(v2(dot_side / 2.0, dot_side / 2.0))?;
        let dot_brush = c.CreateColorBrushWithColor(if theme.light {
            rgba(0, 0, 0, 170)
        } else {
            rgba(255, 255, 255, 215)
        })?;
        let glow_brush = c.CreateColorBrushWithColor(rgba(80, 150, 255, 80))?;
        let glow = (GLOW * s).round();
        for (index, (item, icon)) in items.iter().zip(&self.layout.icons).enumerate() {
            let side = (icon.right - icon.left) as f32;
            // An app's visuals are placed from its icon's top-left corner.
            let app = c.CreateContainerVisual()?;
            let children = app.Children()?;

            let around = v2(side + 2.0 * glow, side + 2.0 * glow);
            let highlight = c.CreateSpriteVisual()?;
            highlight.SetSize(around)?;
            highlight.SetOffset(v3(-glow, -glow))?;
            highlight.SetBrush(&glow_brush)?;
            let shape = c.CreateRoundedRectangleGeometry()?;
            shape.SetSize(around)?;
            shape.SetCornerRadius(v2(around.X * 0.24, around.X * 0.24))?;
            highlight.SetClip(&c.CreateGeometricClipWithGeometry(&shape)?)?;
            highlight.SetOpacity(0.0)?;
            children.InsertAtTop(&highlight)?;

            let picture = c.CreateSpriteVisual()?;
            picture.SetSize(v2(side, side))?;
            if let Some(brush) = &item.icon {
                picture.SetBrush(brush)?;
            }
            children.InsertAtTop(&picture)?;

            if item.running {
                let shape = c.CreateSpriteShapeWithGeometry(&dot)?;
                shape.SetFillBrush(&dot_brush)?;
                let marker = c.CreateShapeVisual()?;
                marker.SetSize(v2(dot_side, dot_side))?;
                marker.Shapes()?.Append(&shape)?;
                marker.SetOffset(v3(side + 3.0 * s, ((side - dot_side) / 2.0).round()))?;
                children.InsertAtTop(&marker)?;
            }

            let margin = glow as i32;
            let mut target = RECT {
                left: icon.left - margin,
                top: icon.top - margin,
                right: icon.right + margin,
                bottom: icon.bottom + margin,
            };
            if self.layout.fan {
                let label = gfx.pill_brush(&item.name, s, theme.light, FAN_LABEL_MAX * s)?;
                let name = c.CreateSpriteVisual()?;
                name.SetSize(v2(label.width, label.height))?;
                name.SetBrush(&label.brush)?;
                let left = -(FAN_LABEL_GAP * s).round() - label.width;
                name.SetOffset(v3(left, ((side - label.height) / 2.0).round()))?;
                children.InsertAtTop(&name)?;
                target.left = target.left.min(icon.left + left.floor() as i32);
            } else if let Some(cell) = self.layout.cells.get(index) {
                let width = (cell.right - cell.left) as f32;
                let label = gfx.text_brush_fitted(
                    &item.name,
                    s,
                    GRID_FONT,
                    false,
                    ink,
                    (width - 8.0 * s).max(1.0),
                )?;
                let name = c.CreateSpriteVisual()?;
                name.SetSize(v2(label.width, label.height))?;
                name.SetBrush(&label.brush)?;
                name.SetOffset(v3(
                    ((width - label.width) / 2.0).round() - (icon.left - cell.left) as f32,
                    side + (GRID_ICON_TOP * s).round(),
                ))?;
                children.InsertAtTop(&name)?;
                target = *cell;
            }
            layers.InsertAtTop(&app)?;
            self.visuals.push(ItemVisual {
                root: app,
                highlight,
                attention: None,
            });
            self.targets.push(target);
        }
        if let Some(target) = &self.target {
            target.SetRoot(&root)?;
        }
        Ok(())
    }

    /// Moves the apps along their opening animation and lights the one
    /// under the pointer.
    pub fn update(&mut self) -> Result<()> {
        let elapsed = self.opened.elapsed().as_secs_f32();
        let hovered = self.hit(self.local_cursor()?);
        let lit = self.pressed.map(|(index, _)| index).or(hovered);
        let start = (
            ((self.anchor.left + self.anchor.right) / 2 - self.layout.bounds.left) as f32,
            (self.anchor.top - self.layout.bounds.top) as f32,
        );
        // The last app starts unfolding last; the panel fades in with the first.
        let last_start = self.visuals.len().saturating_sub(1) as f32 * STAGGER_SECONDS;
        let moving = self.animated && elapsed < last_start + OPEN_SECONDS;
        if moving || self.drawn != Some(lit) {
            self.draw(elapsed, start, lit)?;
        }
        self.drawn = (!moving).then_some(lit);
        // Quick while unfolding; then just often enough to follow the pointer.
        // SAFETY: Our own timer; setting it again replaces it.
        unsafe {
            SetTimer(
                Some(self.hwnd),
                TIMER,
                if moving || self.pressed.is_some() {
                    16
                } else {
                    50
                },
                None,
            )
        };
        Ok(())
    }

    fn draw(&self, elapsed: f32, start: (f32, f32), lit: Option<usize>) -> Result<()> {
        for (index, (visual, icon)) in self.visuals.iter().zip(&self.layout.icons).enumerate() {
            let progress = if self.animated {
                ((elapsed - index as f32 * STAGGER_SECONDS) / OPEN_SECONDS).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let eased = 1.0 - (1.0 - progress).powi(3);
            let side = (icon.right - icon.left) as f32;
            let (left, top) = (icon.left as f32, icon.top as f32);
            // The fan comes out of the group's icon; the grid comes a little
            // way out of the dock.
            let rise = 24.0 * self.scale;
            let (from_x, from_y) = if self.layout.fan {
                (start.0 - side / 2.0, start.1 - side)
            } else {
                match self.edge {
                    DockEdge::Bottom => (left, top + rise),
                    DockEdge::Left => (left - rise, top),
                    DockEdge::Right => (left + rise, top),
                }
            };
            visual.root.SetOffset(v3(
                from_x + (left - from_x) * eased,
                from_y + (top - from_y) * eased,
            ))?;
            visual.root.SetOpacity(eased)?;
            visual
                .highlight
                .SetOpacity(if lit == Some(index) { 1.0 } else { 0.0 })?;
        }
        if let Some(panel) = &self.panel {
            let progress = if self.animated {
                (elapsed / OPEN_SECONDS).min(1.0)
            } else {
                1.0
            };
            panel.SetOpacity(1.0 - (1.0 - progress).powi(3))?;
        }
        Ok(())
    }

    /// `false` once it should close: Escape, a click elsewhere (a click on
    /// the dock excepted: the dock handles it), or another window in front.
    pub fn tick(&mut self) -> Result<bool> {
        let front = tracker::foreground_window();
        if front != self.foreground
            && front != self.dock.0 as isize
            && front != self.hwnd.0 as isize
        {
            return Ok(false);
        }
        let mut point = POINT::default();
        // SAFETY: Read-only pointer, window and key state queries.
        let (escape, mouse_down, over_dock) = unsafe {
            GetCursorPos(&mut point)?;
            (
                GetAsyncKeyState(i32::from(VK_ESCAPE.0)) < 0,
                GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_MBUTTON.0)) < 0,
                WindowFromPoint(point) == self.dock,
            )
        };
        let inside =
            over_dock || contains(self.layout.bounds, point) || contains(self.anchor, point);
        if escape || (mouse_down && !self.mouse_was_down && !inside) {
            return Ok(false);
        }
        self.mouse_was_down = mouse_down;
        // A non-activating popup can miss button-up outside its bounds.
        // Do not leave a stale press that could detach an app on re-entry.
        if self.pressed.is_some() && !app::primary_button_down() {
            self.cancel_press();
        }
        self.update()?;
        Ok(true)
    }

    pub fn press(&mut self) -> Result<()> {
        let mut point = POINT::default();
        // SAFETY: Valid output buffer for a read-only query.
        unsafe { GetCursorPos(&mut point)? };
        self.pressed = self.hit(self.to_local(point)).map(|index| (index, point));
        if self.pressed.is_some() {
            // Keep the mouse while the button is down, wherever it goes: the
            // press may become a drag out of the group.
            // SAFETY: Our own window, on its thread.
            unsafe {
                SetCapture(self.hwnd);
            }
        }
        self.update()
    }

    /// The app pressed, once the pointer has moved far enough to drag it,
    /// and where the pointer is.
    pub fn drag_started(&self) -> Result<Option<(String, POINT)>> {
        let Some((index, at)) = self.pressed else {
            return Ok(None);
        };
        if !app::primary_button_down() {
            return Ok(None);
        }
        let mut point = POINT::default();
        // SAFETY: Read-only queries.
        let threshold = unsafe {
            GetCursorPos(&mut point)?;
            GetSystemMetrics(SM_CXDRAG).max(1)
        };
        let moved = (point.x - at.x).abs() > threshold || (point.y - at.y).abs() > threshold;
        Ok(moved
            .then(|| self.ids.get(index).cloned())
            .flatten()
            .map(|id| (id, point)))
    }

    pub fn release(&mut self) -> Result<Released> {
        let point = self.local_cursor()?;
        let pressed = self.pressed.take();
        if pressed.is_some() {
            // SAFETY: Releasing the capture taken in `press`.
            unsafe {
                let _ = ReleaseCapture();
            }
        }
        let hit = self.hit(point);
        Ok(match pressed {
            Some((index, _)) if hit == Some(index) => self
                .ids
                .get(index)
                .cloned()
                .map_or(Released::Nothing, Released::Item),
            Some(_) => Released::Nothing,
            None if hit.is_none() => Released::Dismiss,
            None => Released::Nothing,
        })
    }

    /// The mouse was taken away mid-press (Alt+Tab, a system dialog).
    pub fn cancel_press(&mut self) {
        self.pressed = None;
        // SAFETY: Release only this popup's capture, never a newer owner's.
        unsafe {
            if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
    }

    /// The app under the pointer, and its icon on screen.
    pub fn item_at_cursor(&self) -> Result<Option<(String, RECT)>> {
        let hit = self.hit(self.local_cursor()?);
        let bounds = self.layout.bounds;
        Ok(hit.and_then(|index| {
            let icon = self.layout.icons.get(index)?;
            Some((
                self.ids.get(index)?.clone(),
                RECT {
                    left: bounds.left + icon.left,
                    top: bounds.top + icon.top,
                    right: bounds.left + icon.right,
                    bottom: bounds.top + icon.bottom,
                },
            ))
        }))
    }

    fn hit(&self, point: POINT) -> Option<usize> {
        self.targets.iter().position(|rect| contains(*rect, point))
    }

    fn to_local(&self, point: POINT) -> POINT {
        POINT {
            x: point.x - self.layout.bounds.left,
            y: point.y - self.layout.bounds.top,
        }
    }

    fn local_cursor(&self) -> Result<POINT> {
        let mut point = POINT::default();
        // SAFETY: Valid output buffer for a read-only query.
        unsafe { GetCursorPos(&mut point)? };
        Ok(self.to_local(point))
    }
}

impl Drop for GroupView {
    fn drop(&mut self) {
        // Release the composition target before the HWND.
        self.target = None;
        // SAFETY: Revoking drop target before destroying the window.
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
            let _ = KillTimer(Some(self.hwnd), TIMER);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn group_view_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if w.0 == TIMER => app::group_view_tick(),
        WM_LBUTTONDOWN => app::group_view_press(),
        WM_MOUSEMOVE => app::group_view_moved(),
        WM_LBUTTONUP => app::group_view_release(),
        WM_RBUTTONUP => app::group_view_menu(),
        // Posted: it also comes while the dock is busy closing this window.
        // SAFETY: Posting to our own window; dropped if it is gone by then.
        WM_CAPTURECHANGED => unsafe {
            let _ = PostMessageW(Some(hwnd), PRESS_LOST, WPARAM(0), LPARAM(0));
        },
        PRESS_LOST if unsafe { GetCapture() } != hwnd => app::group_view_press_lost(),
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
    LRESULT(0)
}

/// An app's icon following the pointer while it is dragged out of a group:
/// above everything, and letting clicks through.
pub struct DragGhost {
    hwnd: HWND,
    /// Kept alive with the window; released before it.
    _target: Option<DesktopWindowTarget>,
    side: i32,
}

impl DragGhost {
    pub fn show(gfx: &Gfx, icon: &CompositionSurfaceBrush, side: f32, at: POINT) -> Result<Self> {
        // SAFETY: Class and window belong to this UI thread; `ghost` owns
        // the HWND as soon as it exists.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("NosihDragGhost");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(ghost_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            });
            // Layered and transparent, as the dock's visual window: drawn
            // by the compositor, and clicks go through it.
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class,
                w!("Nosih"),
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
        let mut ghost = Self {
            hwnd,
            _target: None,
            side: side.round().max(1.0) as i32,
        };
        let length = ghost.side as f32;
        // SAFETY: This process owns the top-level destination window.
        let target = unsafe {
            gfx.compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, true)?
        };
        let root = gfx.compositor.CreateContainerVisual()?;
        let picture = gfx.compositor.CreateSpriteVisual()?;
        picture.SetSize(v2(length, length))?;
        picture.SetBrush(icon)?;
        picture.SetOpacity(0.92)?;
        root.Children()?.InsertAtTop(&picture)?;
        target.SetRoot(&root)?;
        ghost._target = Some(target);
        ghost.move_to(at);
        Ok(ghost)
    }

    /// Centers the icon on `at`, on screen.
    pub fn move_to(&self, at: POINT) {
        // SAFETY: Our own window; moving it never activates it.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                at.x - self.side / 2,
                at.y - self.side / 2,
                self.side,
                self.side,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

impl Drop for DragGhost {
    fn drop(&mut self) {
        self._target = None;
        // SAFETY: Our own window, on its thread.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn ghost_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    fn anchor(left: i32) -> RECT {
        RECT {
            left,
            top: 990,
            right: left + 48,
            bottom: 1038,
        }
    }

    #[test]
    fn a_few_apps_should_unfold_as_a_fan_going_up_from_the_group() {
        let layout = GroupLayout::new(SCREEN, anchor(900), DockEdge::Bottom, 1.0, 48.0, 4);
        assert!(layout.fan);
        assert!(layout.bounds.bottom <= anchor(900).top);
        let tops: Vec<i32> = layout.icons.iter().map(|icon| icon.top).collect();
        assert!(tops.windows(2).all(|pair| pair[1] < pair[0]));
        // The first app sits right above the group.
        let first = layout.icons[0];
        let middle = layout.bounds.left + (first.left + first.right) / 2;
        assert!((middle - 924).abs() <= 1);
    }

    #[test]
    fn many_apps_or_a_short_screen_should_unfold_as_a_grid() {
        assert!(!GroupLayout::new(SCREEN, anchor(900), DockEdge::Bottom, 1.0, 48.0, 30).fan);
        let short = RECT {
            bottom: 400,
            ..SCREEN
        };
        let low = RECT {
            top: 340,
            bottom: 388,
            ..anchor(900)
        };
        assert!(!GroupLayout::new(short, low, DockEdge::Bottom, 1.0, 48.0, 8).fan);
    }

    #[test]
    fn group_layout_should_stay_on_screen_at_both_edges_and_high_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            for left in [0, 900, 1872] {
                for count in [1, 3, 10, 40] {
                    let layout = GroupLayout::new(
                        SCREEN,
                        anchor(left),
                        DockEdge::Bottom,
                        scale,
                        48.0 * scale,
                        count,
                    );
                    let bounds = layout.bounds;
                    assert!(bounds.left >= SCREEN.left && bounds.right <= SCREEN.right);
                    assert!(bounds.top >= SCREEN.top && bounds.bottom <= anchor(left).top);
                    let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
                    for icon in &layout.icons {
                        assert!(icon.left >= 0 && icon.right <= width);
                        assert!(icon.top >= 0 && icon.bottom <= height);
                    }
                }
            }
        }
    }

    #[test]
    fn on_a_side_dock_the_group_opens_beside_it_and_on_screen() {
        for (edge, icon) in [
            (
                DockEdge::Left,
                RECT {
                    left: 14,
                    top: 500,
                    right: 62,
                    bottom: 548,
                },
            ),
            (
                DockEdge::Right,
                RECT {
                    left: 1858,
                    top: 500,
                    right: 1906,
                    bottom: 548,
                },
            ),
        ] {
            for count in [2, 9, 60] {
                let layout = GroupLayout::new(SCREEN, icon, edge, 1.25, 60.0, count);
                let bounds = layout.bounds;
                assert!(!layout.fan);
                assert!(bounds.top >= SCREEN.top && bounds.bottom <= SCREEN.bottom);
                if edge == DockEdge::Left {
                    assert!(bounds.left >= icon.right && bounds.right <= SCREEN.right);
                } else {
                    assert!(bounds.right <= icon.left && bounds.left >= SCREEN.left);
                }
            }
        }
    }

    #[test]
    fn grid_cells_should_not_overlap() {
        let layout = GroupLayout::new(SCREEN, anchor(900), DockEdge::Bottom, 1.0, 48.0, 23);
        assert_eq!(layout.cells.len(), 23);
        for (index, cell) in layout.cells.iter().enumerate() {
            let center = POINT {
                x: (cell.left + cell.right) / 2,
                y: (cell.top + cell.bottom) / 2,
            };
            let under: Vec<usize> = layout
                .cells
                .iter()
                .enumerate()
                .filter(|(_, other)| contains(**other, center))
                .map(|(other, _)| other)
                .collect();
            assert_eq!(under, [index]);
        }
    }
}
