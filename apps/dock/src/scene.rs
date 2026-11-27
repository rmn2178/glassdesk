//! Dock visual tree: shadow, glass, specular border, dividers, icons, indicators,
//! and labels. It is built once per configuration; only its elements move per frame.

#[cfg(test)]
#[path = "scene/material_tests.rs"]
mod material_tests;

use std::{cell::Cell, rc::Rc};

use anyhow::Result;
use nosih_core::{
    frame::{DockEdge, Frame as DockFrame},
    layout::{ItemKind, Layout, Rect},
    magnify::Magnified,
};
use windows::{
    UI::{
        Color,
        Composition::{
            CompositionBrush, CompositionColorSpace, CompositionRoundedRectangleGeometry,
            CompositionStretch, CompositionSurfaceBrush, ContainerVisual, ShapeVisual,
            SpriteVisual, Visual,
        },
    },
    core::Interface,
};
use windows_numerics::{Vector2, Vector3};

use crate::{
    appearance::{Look, Material},
    gfx::{Gfx, LabelBrush},
};
use nosih_shell::icons::IconImage;

#[derive(Clone, Copy)]
pub struct Theme {
    pub light: bool,
    /// Blur what is behind the dock; without it the tint is drawn opaque.
    glass: bool,
    tint: Color,
    /// A neutral, static optical coating; no capture, shader or animation loop.
    lens: bool,
    /// Folders and window cards hold text: a clear dock still gets a
    /// panel that keeps it readable.
    panel_tint: Color,
    /// Top of the light that falls on the glass, fading by the middle.
    sheen: u8,
    /// The rim's brightness at its corners and sides (see `GlassSurface`).
    rim: [u8; 4],
    separator: Color,
    shadow: Color,
    indicator: Color,
}

/// macOS Tahoe 26 Liquid Glass optical coating: crisp upper Fresnel meniscus,
/// crystal-clear refractive centre, subtle glass-depth absorption, and lower
/// internal caustic bounce. Allocated only when a surface is built.
const LENS_COATING: [(f32, Color); 5] = [
    (0.0, rgba(255, 255, 255, 96)),
    (0.18, rgba(255, 255, 255, 18)),
    (0.48, rgba(255, 255, 255, 0)),
    (0.78, rgba(0, 0, 0, 26)),
    (1.0, rgba(255, 255, 255, 64)),
];

impl Theme {
    /// Without `look.glass` (Windows transparency off, or energy saver on)
    /// every material is solid, as macOS draws it with "Reduce transparency".
    pub fn new(look: Look) -> Self {
        let Look {
            material,
            light,
            glass,
        } = look;
        let pick = |clear: Color, solid: Color| if glass { clear } else { solid };
        let tint = match (material, light) {
            (Material::Standard, true) => pick(rgba(255, 255, 255, 46), rgba(226, 226, 228, 255)),
            (Material::Standard, false) => pick(rgba(28, 28, 30, 82), rgba(44, 44, 46, 255)),
            (Material::Frosted, true) => pick(rgba(250, 251, 253, 104), rgba(236, 238, 242, 255)),
            (Material::Frosted, false) => pick(rgba(34, 38, 46, 144), rgba(40, 44, 52, 255)),
            (Material::Clear, _) => pick(rgba(255, 255, 255, 14), rgba(38, 40, 46, 255)),
            (Material::LiquidGlass, true) => {
                pick(rgba(248, 251, 255, 28), rgba(230, 234, 240, 255))
            }
            (Material::LiquidGlass, false) => pick(rgba(18, 24, 34, 54), rgba(34, 40, 50, 255)),
        };
        let (sheen, rim) = match (material, light) {
            (Material::Standard, _) => (38, [150, 40, 30, 105]),
            (Material::Frosted, true) => (46, [190, 60, 50, 135]),
            (Material::Frosted, false) => (30, [115, 35, 28, 90]),
            // All the light is on the edge, like a lens.
            (Material::Clear, _) => (56, [235, 80, 60, 190]),
            (Material::LiquidGlass, _) if !glass => (0, [100, 30, 25, 70]),
            (Material::LiquidGlass, true) => (88, [255, 110, 32, 238]),
            (Material::LiquidGlass, false) => (72, [235, 84, 24, 205]),
        };
        let panel_tint = match material {
            // Text-bearing panels keep a denser surface than the icon-only
            // dock. Reducing the dock's tint must not wash out popup content.
            Material::Frosted if light => pick(rgba(250, 251, 253, 212), tint),
            Material::Frosted => pick(rgba(30, 34, 42, 212), tint),
            Material::Clear => pick(rgba(24, 24, 28, 150), tint),
            Material::LiquidGlass if light => pick(rgba(245, 249, 255, 208), tint),
            Material::LiquidGlass => pick(rgba(26, 32, 42, 208), tint),
            _ => tint,
        };
        Self {
            light,
            glass,
            tint,
            lens: material == Material::LiquidGlass && glass,
            panel_tint,
            sheen,
            rim,
            separator: if light {
                rgba(0, 0, 0, 56)
            } else {
                rgba(255, 255, 255, 64)
            },
            shadow: if light {
                rgba(0, 0, 0, 56)
            } else {
                rgba(0, 0, 0, 96)
            },
            indicator: if light {
                rgba(0, 0, 0, 170)
            } else {
                rgba(255, 255, 255, 215)
            },
        }
    }

    /// The menu bar: the same material, a little denser so that the clock
    /// and the status items stay readable over any wallpaper.
    pub(crate) fn for_bar(look: Look) -> Self {
        let pick = |clear: Color, solid: Color| if look.glass { clear } else { solid };
        let tint = match (look.material, look.light) {
            (Material::Standard, true) => pick(rgba(255, 255, 255, 110), rgba(236, 236, 238, 255)),
            (Material::Standard, false) => pick(rgba(24, 24, 26, 120), rgba(36, 36, 38, 255)),
            (Material::Frosted, true) => pick(rgba(250, 251, 253, 136), rgba(238, 240, 244, 255)),
            (Material::Frosted, false) => pick(rgba(28, 32, 40, 184), rgba(38, 42, 50, 255)),
            (Material::Clear, _) => pick(rgba(0, 0, 0, 38), rgba(30, 32, 38, 255)),
            (Material::LiquidGlass, true) => {
                pick(rgba(248, 251, 255, 44), rgba(236, 240, 246, 255))
            }
            (Material::LiquidGlass, false) => pick(rgba(18, 24, 34, 56), rgba(30, 36, 46, 255)),
        };
        Self {
            tint,
            ..Self::new(look)
        }
    }

    /// Brushes that fill a surface of this theme, bottom first: what is
    /// behind it (with glass), the tint, and an optional lens coating.
    pub(crate) fn fills(&self, gfx: &Gfx) -> Result<Vec<CompositionBrush>> {
        self.fills_on(gfx, DockEdge::Bottom)
    }

    fn fills_on(&self, gfx: &Gfx, edge: DockEdge) -> Result<Vec<CompositionBrush>> {
        let c = &gfx.compositor;
        // Leave room for GlassSurface's sheen too, so building it never grows
        // the buffer after adding the optional coating.
        let mut fills = Vec::with_capacity(4);
        if self.glass {
            fills.push(c.CreateHostBackdropBrush()?.cast()?);
        }
        fills.push(c.CreateColorBrushWithColor(self.tint)?.cast()?);
        if self.lens {
            let coating = c.CreateLinearGradientBrush()?;
            let (start, end) = reflection_axis(edge);
            coating.SetStartPoint(start)?;
            coating.SetEndPoint(end)?;
            let stops = coating.ColorStops()?;
            for (at, color) in LENS_COATING {
                stops.Append(&c.CreateColorGradientStopWithOffsetAndColor(at, color)?)?;
            }
            fills.push(coating.cast()?);
        }
        Ok(fills)
    }

    /// Inside the frame around the icon a dragged app would join.
    fn merge_fill(&self) -> Color {
        if self.light {
            rgba(255, 255, 255, 120)
        } else {
            rgba(255, 255, 255, 46)
        }
    }

    /// Uses the context menu tint with the same rim and sheen as the window picker.
    pub(crate) fn for_popup(light: bool, glass: bool) -> Self {
        let tint = crate::menu::popup_tint(!light, glass);
        Self {
            tint,
            panel_tint: tint,
            ..Self::new(Look {
                material: Material::Standard,
                light,
                glass,
            })
        }
    }

    /// Popup surface that preserves Liquid Glass (optical lens coating, sheen,
    /// bright curved rim, and translucent backdrop blur) while keeping text readable.
    pub(crate) fn for_popup_look(look: Look) -> Self {
        if look.material == Material::LiquidGlass {
            Self::new(look)
        } else {
            Self::for_popup(look.light, look.glass)
        }
    }

    /// A fixed-size panel using the dock's backdrop, tint, sheen and rim.
    pub fn panel(
        &self,
        gfx: &Gfx,
        size: Vector2,
        radius: f32,
        scale: f32,
    ) -> Result<ContainerVisual> {
        let stroke = scale.max(1.0);
        let panel = Self {
            tint: self.panel_tint,
            ..*self
        };
        let material = GlassSurface::build(gfx, panel, radius, stroke, DockEdge::Bottom)?;
        material.glass.SetSize(size)?;
        material.outline.SetSize(size)?;
        material.rim.SetSize(size)?;
        material
            .rim_geometry
            .SetSize(v2(size.X - stroke, size.Y - stroke))?;
        if let Some(inner) = &material.inner_rim_geometry {
            let inset = stroke * 3.0;
            inner.SetSize(v2((size.X - inset).max(0.0), (size.Y - inset).max(0.0)))?;
        }
        let root = gfx.compositor.CreateContainerVisual()?;
        root.SetSize(size)?;
        root.Children()?.InsertAtTop(&material.glass)?;
        root.Children()?.InsertAtTop(&material.rim)?;
        Ok(root)
    }
}

/// The material is shared so dock and preview panels follow the same theme
/// and opaque fallback when Windows transparency is disabled.
struct GlassSurface {
    glass: ContainerVisual,
    outline: CompositionRoundedRectangleGeometry,
    rim: ShapeVisual,
    rim_geometry: CompositionRoundedRectangleGeometry,
    inner_rim_geometry: Option<CompositionRoundedRectangleGeometry>,
}

impl GlassSurface {
    fn build(gfx: &Gfx, theme: Theme, radius: f32, stroke: f32, edge: DockEdge) -> Result<Self> {
        let c = &gfx.compositor;
        let glass = c.CreateContainerVisual()?;
        let outline = c.CreateRoundedRectangleGeometry()?;
        outline.SetCornerRadius(v2(radius, radius))?;
        glass.SetClip(&c.CreateGeometricClipWithGeometry(&outline)?)?;

        let sheen = c.CreateLinearGradientBrush()?;
        let (start, end) = reflection_axis(if theme.lens { edge } else { DockEdge::Bottom });
        sheen.SetStartPoint(start)?;
        sheen.SetEndPoint(end)?;
        let stops = sheen.ColorStops()?;
        stops.Append(
            &c.CreateColorGradientStopWithOffsetAndColor(0.0, rgba(255, 255, 255, theme.sheen))?,
        )?;
        let falloff = if theme.lens { 0.22 } else { 0.55 };
        stops.Append(
            &c.CreateColorGradientStopWithOffsetAndColor(falloff, rgba(255, 255, 255, 0))?,
        )?;

        let mut fills = theme.fills_on(gfx, edge)?;
        fills.push(sheen.cast()?);
        for brush in &fills {
            let fill = c.CreateSpriteVisual()?;
            fill.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            fill.SetBrush(brush)?;
            glass.Children()?.InsertAtTop(&fill)?;
        }

        let rim_brush = c.CreateLinearGradientBrush()?;
        rim_brush.SetStartPoint(v2(0.0, 0.0))?;
        rim_brush.SetEndPoint(v2(1.0, 1.0))?;
        rim_brush.SetInterpolationSpace(CompositionColorSpace::RgbLinear)?;
        let stops = rim_brush.ColorStops()?;
        for (at, alpha) in [0.0, 0.35, 0.65, 1.0].into_iter().zip(theme.rim) {
            stops.Append(
                &c.CreateColorGradientStopWithOffsetAndColor(at, rgba(255, 255, 255, alpha))?,
            )?;
        }
        let rim_geometry = c.CreateRoundedRectangleGeometry()?;
        rim_geometry.SetOffset(v2(stroke / 2.0, stroke / 2.0))?;
        rim_geometry.SetCornerRadius(v2(radius - stroke / 2.0, radius - stroke / 2.0))?;
        let rim_shape = c.CreateSpriteShapeWithGeometry(&rim_geometry)?;
        rim_shape.SetStrokeBrush(&rim_brush)?;
        rim_shape.SetStrokeThickness(stroke)?;
        let rim = c.CreateShapeVisual()?;
        rim.Shapes()?.Append(&rim_shape)?;
        let inner_rim_geometry = if theme.lens {
            // Concentric inner optical bevel lift (white 28% top -> clear -> 15% bottom caustic).
            let inner_brush = c.CreateLinearGradientBrush()?;
            let (start, end) = reflection_axis(edge);
            inner_brush.SetStartPoint(start)?;
            inner_brush.SetEndPoint(end)?;
            let inner_stops = inner_brush.ColorStops()?;
            for (at, alpha) in [(0.0, 72u8), (0.45, 0u8), (1.0, 38u8)] {
                inner_stops.Append(
                    &c.CreateColorGradientStopWithOffsetAndColor(at, rgba(255, 255, 255, alpha))?,
                )?;
            }
            let inner_geo = c.CreateRoundedRectangleGeometry()?;
            let inset = stroke * 1.5;
            inner_geo.SetOffset(v2(inset, inset))?;
            inner_geo.SetCornerRadius(v2((radius - inset).max(0.0), (radius - inset).max(0.0)))?;
            let inner_shape = c.CreateSpriteShapeWithGeometry(&inner_geo)?;
            inner_shape.SetStrokeBrush(&inner_brush)?;
            inner_shape.SetStrokeThickness(stroke)?;
            rim.Shapes()?.Append(&inner_shape)?;
            Some(inner_geo)
        } else {
            None
        };
        Ok(Self {
            glass,
            outline,
            rim,
            rim_geometry,
            inner_rim_geometry,
        })
    }
}

/// Outline of the icon a dragged app would join in a group.
const MERGE_ACCENT: Color = rgba(80, 150, 255, 230);

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

/// The highlight follows the dock's depth, even when its long axis is vertical.
fn reflection_axis(edge: DockEdge) -> (Vector2, Vector2) {
    match edge {
        DockEdge::Bottom => (v2(0.0, 0.0), v2(0.0, 1.0)),
        DockEdge::Left => (v2(1.0, 0.0), v2(0.0, 0.0)),
        DockEdge::Right => (v2(0.0, 0.0), v2(1.0, 0.0)),
    }
}

/// Precomputed 64×64 radial optical highlight for pointer-reactive Liquid Glass
/// specular sheen on the dock (16 KB GPU texture, zero idle CPU overhead).
fn specular_glint_brush(gfx: &Gfx, light: bool) -> Result<CompositionSurfaceBrush> {
    const SIDE: u32 = 64;
    let peak = if light { 84.0f32 } else { 68.0f32 };
    let center = (SIDE as f32 - 1.0) / 2.0;
    let inv_r = 1.0 / center;
    let mut bgra = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        let dy = (y as f32 - center) * inv_r;
        for x in 0..SIDE {
            let dx = (x as f32 - center) * inv_r;
            let r2 = dx * dx + dy * dy;
            let alpha = if r2 >= 1.0 {
                0u8
            } else {
                let falloff = (1.0 - r2) * (1.0 - r2);
                (peak * falloff).round().clamp(0.0, 255.0) as u8
            };
            // Premultiplied white in BGRA.
            bgra.extend_from_slice(&[alpha, alpha, alpha, alpha]);
        }
    }
    let image = IconImage {
        width: SIDE,
        height: SIDE,
        bgra,
    };
    let surface = gfx.image_surface(&image)?;
    let brush = gfx.compositor.CreateSurfaceBrushWithSurface(&surface)?;
    brush.SetStretch(CompositionStretch::Fill)?;
    Ok(brush)
}

#[derive(Clone, Copy, Default)]
pub struct SlotContent<'a> {
    pub icon: Option<&'a CompositionSurfaceBrush>,
    pub label: Option<&'a LabelBrush>,
    pub running: bool,
    /// App icon shown in the corner of a minimized-window thumbnail.
    pub badge: Option<&'a CompositionSurfaceBrush>,
}

pub struct SceneInput<'a> {
    pub layout: &'a Layout,
    pub contents: &'a [SlotContent<'a>],
    pub theme: Theme,
    pub scale: f32,
    /// Where the resting dock sits in the visual window, and on which edge:
    /// every position the scene draws goes through it.
    pub frame: DockFrame,
    /// Shown above an icon dragged out of the dock.
    pub remove_label: Option<&'a LabelBrush>,
}

/// App badge on a minimized-window thumbnail: side relative to the slot, and
/// how much of it lies inside the slot.
pub const BADGE_SIZE: f32 = 0.42;
pub const BADGE_INSIDE: f32 = 0.85;

/// A native attention request, deliberately without a fabricated unread count.
/// Its vector is allocated on first use and only transformed during animation.
pub(crate) struct AttentionBadge {
    pub(crate) visual: ShapeVisual,
    visible: bool,
}

impl AttentionBadge {
    pub(crate) fn new(gfx: &Gfx, theme: Theme) -> Result<Self> {
        let c = &gfx.compositor;
        let circle = c.CreateEllipseGeometry()?;
        circle.SetCenter(v2(0.5, 0.5))?;
        circle.SetRadius(v2(0.43, 0.43))?;
        let shape = c.CreateSpriteShapeWithGeometry(&circle)?;
        shape.SetFillBrush(&c.CreateColorBrushWithColor(if theme.light {
            rgba(255, 59, 48, 255)
        } else {
            rgba(255, 69, 58, 255)
        })?)?;
        shape.SetStrokeBrush(&c.CreateColorBrushWithColor(if theme.light {
            rgba(255, 255, 255, 245)
        } else {
            rgba(38, 38, 40, 245)
        })?)?;
        shape.SetStrokeThickness(0.12)?;
        let visual = c.CreateShapeVisual()?;
        visual.SetSize(v2(1.0, 1.0))?;
        visual.Shapes()?.Append(&shape)?;
        visual.SetIsVisible(false)?;
        Ok(Self {
            visual,
            visible: false,
        })
    }

    pub(crate) fn set_visible(&mut self, visible: bool) -> Result<()> {
        if self.visible != visible {
            self.visual.SetIsVisible(visible)?;
            self.visible = visible;
        }
        Ok(())
    }

    pub(crate) fn place(&self, icon: Rect, scale: f32, opacity: f32, snap: bool) -> Result<()> {
        if !self.visible {
            return Ok(());
        }
        let rect = attention_rect(icon, scale);
        let fit = |value: f32| if snap { value.round() } else { value };
        self.visual
            .SetScale(Vector3::new(fit(rect.width), fit(rect.height), 1.0))?;
        self.visual.SetOffset(v3(fit(rect.x), fit(rect.y)))?;
        self.visual.SetOpacity(opacity)?;
        Ok(())
    }
}

/// Where an app's own overlay icon goes (issue #65): its unread count or
/// status, at the top right of the dock icon as a macOS badge, a little
/// past its corner so it reads as on top of the icon.
fn overlay_rect(icon: Rect) -> Rect {
    let side = icon.width.min(icon.height) * 0.42;
    Rect {
        x: icon.x + icon.width - side * 0.85,
        y: icon.y - side * 0.15,
        width: side,
        height: side,
    }
}

fn attention_rect(icon: Rect, scale: f32) -> Rect {
    let side = (icon.width * 0.25)
        .clamp(8.0 * scale, 14.0 * scale)
        .min(icon.width.min(icon.height) * 0.45);
    let inset = scale.min((icon.width - side) / 2.0).max(0.0);
    Rect {
        x: icon.x + icon.width - side - inset,
        y: icon.y + inset,
        width: side,
        height: side,
    }
}

#[cfg(test)]
mod attention_tests {
    use super::*;
    use nosih_core::{
        layout::{Metrics, layout},
        magnify::{Magnifier, magnify},
    };

    #[test]
    fn attention_stays_inside_small_magnified_and_high_dpi_icons() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            for width in [16.0, 32.0, 48.0, 96.0] {
                let icon = Rect {
                    x: 17.0,
                    y: -32.0,
                    width: width * scale,
                    height: width * scale,
                };
                let overlay = overlay_rect(icon);
                assert!(
                    overlay.width > 0.0 && overlay.x + overlay.width > icon.x + icon.width * 0.8
                );
                assert!(overlay.y <= icon.y && overlay.y + overlay.height > icon.y);
                let badge = attention_rect(icon, scale);
                assert!(badge.x >= icon.x && badge.y >= icon.y);
                assert!(badge.x + badge.width <= icon.x + icon.width);
                assert!(badge.y + badge.height <= icon.y + icon.height);
                assert!(badge.width > 0.0);
            }
        }
    }

    #[test]
    #[ignore = "Requires Windows desktop composition"]
    fn native_app_overlays_keep_their_visual_follow_the_icon_and_go() -> Result<()> {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        let scale = 1.25;
        let layout = layout(
            &[ItemKind::Icon, ItemKind::Icon],
            &Metrics::DEFAULT.scaled(scale),
        );
        let contents = [SlotContent::default(); 2];
        let mut scene = Scene::build(
            &gfx,
            &SceneInput {
                layout: &layout,
                contents: &contents,
                theme: Theme::new(Look {
                    material: crate::appearance::Material::Standard,
                    light: true,
                    glass: true,
                }),
                scale,
                frame: DockFrame::new(DockEdge::Bottom, (0.0, 0.0), layout.height),
                remove_label: None,
            },
        )?;
        let root = scene.root.clone();
        // A red dot with an unread count, as Slack sends it.
        let image = Rc::new(IconImage {
            width: 16,
            height: 16,
            bgra: [60u8, 59, 255, 255].repeat(256),
        });
        scene.set_overlays(&gfx, [Some(image.clone()), None].into_iter())?;
        let visual = scene.overlays[0]
            .as_ref()
            .map(|(_, visual)| visual.clone())
            .ok_or_else(|| anyhow::anyhow!("missing overlay"))?;
        assert!(scene.overlays[1].is_none());
        // The same image again keeps its visual.
        scene.set_overlays(&gfx, [Some(image), None].into_iter())?;
        assert!(
            scene.overlays[0]
                .as_ref()
                .is_some_and(|(_, kept)| *kept == visual)
        );
        let state = magnify(
            &layout,
            24.0 * scale,
            1.0,
            &Magnifier {
                range: 144.0 * scale,
                max_scale: 1.6,
            },
        );
        scene.apply(&Frame {
            state: &state,
            hovered: None,
            lifts: &[0.0, 0.0],
            drag: None,
            target: None,
            slide: 0.0,
            snap: true,
        })?;
        let icon = state.rects[0];
        assert!(visual.Size()?.X > 0.0);
        assert!(visual.Offset()?.X > icon.x);
        scene.set_overlays(&gfx, [None, None].into_iter())?;
        assert!(scene.overlays.iter().all(Option::is_none));
        assert!(scene.root == root);
        Ok(())
    }

    #[test]
    #[ignore = "Requires Windows desktop composition; creates no visible window"]
    fn native_attention_badges_reuse_visuals_follow_drag_and_hide_without_rebuilding() -> Result<()>
    {
        use windows::Win32::System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        };
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?;
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (material, light) in crate::appearance::Material::ALL
                .into_iter()
                .flat_map(|material| [(material, false), (material, true)])
            {
                let layout = layout(
                    &[ItemKind::Icon, ItemKind::Icon],
                    &Metrics::DEFAULT.scaled(scale),
                );
                let contents = [SlotContent::default(); 2];
                let mut scene = Scene::build(
                    &gfx,
                    &SceneInput {
                        layout: &layout,
                        contents: &contents,
                        theme: Theme::new(Look {
                            material,
                            light,
                            glass: true,
                        }),
                        scale,
                        frame: DockFrame::new(DockEdge::Bottom, (0.0, 0.0), layout.height),
                        remove_label: None,
                    },
                )?;
                let root = scene.root.clone();
                scene.set_attention(&gfx, [true, false].into_iter())?;
                let visual = scene.attention[0]
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing badge"))?
                    .visual
                    .clone();
                scene.set_attention(&gfx, [true, false].into_iter())?;
                assert!(
                    scene.attention[0]
                        .as_ref()
                        .is_some_and(|badge| badge.visual == visual)
                );
                assert!(scene.attention[1].is_none());
                let state = magnify(
                    &layout,
                    24.0 * scale,
                    1.0,
                    &Magnifier {
                        range: 144.0 * scale,
                        max_scale: 1.6,
                    },
                );
                scene.apply(&Frame {
                    state: &state,
                    hovered: None,
                    lifts: &[8.0 * scale, 0.0],
                    drag: Some(Drag {
                        index: 0,
                        center: (80.0 * scale, 20.0 * scale),
                        removing: false,
                        ghost: false,
                    }),
                    target: None,
                    slide: 0.0,
                    snap: false,
                })?;
                assert!(visual.IsVisible()?);
                assert!(visual.Scale()?.X > 0.0);
                scene.set_attention(&gfx, [false, false].into_iter())?;
                assert!(!visual.IsVisible()?);
                assert!(scene.root == root);
            }
        }
        Ok(())
    }
}

/// An icon following the cursor while it is dragged.
#[derive(Clone, Copy)]
pub struct Drag {
    pub index: usize,
    /// Icon center in dock coordinates (along, across the dock).
    pub center: (f32, f32),
    /// Released here, the icon would leave the dock.
    pub removing: bool,
    /// Taken out of a group: a window of its own follows the pointer (see
    /// `group_view::DragGhost`), and the slot only shows, faded, where the
    /// app will land.
    pub ghost: bool,
}

/// Everything that changes from one frame to the next.
pub struct Frame<'a> {
    pub state: &'a Magnified,
    pub hovered: Option<usize>,
    pub lifts: &'a [f32],
    pub drag: Option<Drag>,
    /// The icon the dragged app would join in a group if released now.
    pub target: Option<usize>,
    /// How far the whole dock has slid toward the screen edge (auto-hide),
    /// in pixels.
    pub slide: f32,
    /// Align to whole pixels; only at rest, where sharpness matters.
    pub snap: bool,
}

struct Label {
    visual: SpriteVisual,
    width: f32,
    height: f32,
}

impl Label {
    fn new(c: &windows::UI::Composition::Compositor, brush: &LabelBrush) -> Result<Self> {
        let visual = c.CreateSpriteVisual()?;
        visual.SetSize(v2(brush.width, brush.height))?;
        visual.SetBrush(&brush.brush)?;
        visual.SetIsVisible(false)?;
        Ok(Self {
            visual,
            width: brush.width,
            height: brush.height,
        })
    }
}

pub struct Scene {
    pub root: ContainerVisual,
    frame: DockFrame,
    height: f32,
    stroke: f32,
    label_gap: f32,
    indicator_size: f32,
    indicator_y: f32,
    frames: [Visual; 3],
    outline: CompositionRoundedRectangleGeometry,
    rim_geometry: CompositionRoundedRectangleGeometry,
    inner_rim_geometry: Option<CompositionRoundedRectangleGeometry>,
    /// Pointer-reactive Liquid Glass specular highlight clipped inside `glass`.
    glint: Option<SpriteVisual>,
    content: ContainerVisual,
    /// Frame drawn around the icon a dragged app would join.
    merge: ShapeVisual,
    merge_geometry: CompositionRoundedRectangleGeometry,
    merge_stroke: f32,
    /// The slot brought to the front for the current drag, so it passes
    /// over the icons it is dropped on.
    raised: Cell<Option<usize>>,
    slots: Vec<SpriteVisual>,
    badges: Vec<Option<SpriteVisual>>,
    attention: Vec<Option<AttentionBadge>>,
    /// Each app's own overlay icon, from its taskbar button, and the image
    /// it shows (see `overlays`).
    overlays: Vec<Option<(Rc<IconImage>, SpriteVisual)>>,
    attention_theme: Theme,
    scale: f32,
    indicators: Vec<Option<ShapeVisual>>,
    labels: Vec<Option<Label>>,
    remove_label: Option<Label>,
}

impl Scene {
    /// Where the slot is drawn now, along the dock, in dock coordinates.
    pub fn slot_along(&self, index: usize) -> Option<f32> {
        let offset = self.slots.get(index)?.Offset().ok()?;
        let size = self.slots.get(index)?.Size().ok()?;
        // The box's top-left maps from a different dock corner on each edge;
        // its centre maps from its centre on all of them.
        let (along, _) = self
            .frame
            .to_dock(offset.X + size.X / 2.0, offset.Y + size.Y / 2.0);
        let length = if self.frame.edge.is_vertical() {
            size.Y
        } else {
            size.X
        };
        Some(along - length / 2.0)
    }

    pub fn build(gfx: &Gfx, input: &SceneInput<'_>) -> Result<Self> {
        let c = &gfx.compositor;
        let SceneInput {
            layout,
            theme,
            scale,
            frame,
            ..
        } = *input;
        let radius = layout.height * 0.36;

        let root = c.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        let layers = root.Children()?;

        let shadow = c.CreateDropShadow()?;
        let mask = c.CreateNineGridBrush()?;
        mask.SetSource(&gfx.rounded_mask(radius)?)?;
        mask.SetInsets(radius.ceil() + 1.0)?;
        shadow.SetMask(&mask)?;
        shadow.SetBlurRadius(22.0 * scale)?;
        shadow.SetOffset(v3(0.0, 8.0 * scale))?;
        shadow.SetColor(theme.shadow)?;
        let shadow_host = c.CreateSpriteVisual()?;
        shadow_host.SetShadow(&shadow)?;
        layers.InsertAtTop(&shadow_host)?;

        let stroke = scale.max(1.0);
        let GlassSurface {
            glass,
            outline,
            rim,
            rim_geometry,
            inner_rim_geometry,
        } = GlassSurface::build(gfx, theme, radius, stroke, frame.edge)?;
        let glint = if theme.lens {
            let brush = specular_glint_brush(gfx, theme.light)?;
            let visual = c.CreateSpriteVisual()?;
            let glint_size = layout.height * 2.2;
            visual.SetSize(v2(glint_size, glint_size))?;
            visual.SetBrush(&brush)?;
            visual.SetOpacity(0.0)?;
            glass.Children()?.InsertAtTop(&visual)?;
            Some(visual)
        } else {
            None
        };
        layers.InsertAtTop(&glass)?;
        layers.InsertAtTop(&rim)?;

        // Children are placed in window coordinates through `frame`.
        let content = c.CreateContainerVisual()?;
        let children = content.Children()?;
        let indicator_size = (4.0 * scale).round();
        let dot = c.CreateEllipseGeometry()?;
        dot.SetCenter(v2(indicator_size / 2.0, indicator_size / 2.0))?;
        dot.SetRadius(v2(indicator_size / 2.0, indicator_size / 2.0))?;
        let dot_brush = c.CreateColorBrushWithColor(theme.indicator)?;

        // Below the icons: it frames the one under the dragged app.
        let merge_stroke = (2.0 * scale).max(1.0);
        let merge_geometry = c.CreateRoundedRectangleGeometry()?;
        merge_geometry.SetOffset(v2(merge_stroke / 2.0, merge_stroke / 2.0))?;
        let merge_shape = c.CreateSpriteShapeWithGeometry(&merge_geometry)?;
        merge_shape.SetFillBrush(&c.CreateColorBrushWithColor(theme.merge_fill())?)?;
        merge_shape.SetStrokeBrush(&c.CreateColorBrushWithColor(MERGE_ACCENT)?)?;
        merge_shape.SetStrokeThickness(merge_stroke)?;
        let merge = c.CreateShapeVisual()?;
        merge.Shapes()?.Append(&merge_shape)?;
        merge.SetIsVisible(false)?;
        children.InsertAtTop(&merge)?;

        let mut slots = Vec::with_capacity(layout.slots.len());
        let mut badges = Vec::with_capacity(layout.slots.len());
        let mut indicators = Vec::with_capacity(layout.slots.len());
        let mut labels = Vec::with_capacity(layout.slots.len());
        for (slot, item) in layout.slots.iter().zip(input.contents) {
            let visual = c.CreateSpriteVisual()?;
            match (slot.kind, item.icon) {
                (ItemKind::Separator, _) => {
                    visual.SetBrush(&c.CreateColorBrushWithColor(theme.separator)?)?;
                }
                (ItemKind::Icon, Some(icon)) => visual.SetBrush(icon)?,
                (ItemKind::Icon, None) => {}
            }
            children.InsertAtTop(&visual)?;
            slots.push(visual);
            badges.push(match item.badge {
                Some(brush) => {
                    let badge = c.CreateSpriteVisual()?;
                    badge.SetBrush(brush)?;
                    children.InsertAtTop(&badge)?;
                    Some(badge)
                }
                None => None,
            });

            indicators.push(if item.running {
                let shape = c.CreateSpriteShapeWithGeometry(&dot)?;
                shape.SetFillBrush(&dot_brush)?;
                let visual = c.CreateShapeVisual()?;
                visual.SetSize(v2(indicator_size, indicator_size))?;
                visual.Shapes()?.Append(&shape)?;
                children.InsertAtTop(&visual)?;
                Some(visual)
            } else {
                None
            });

            labels.push(item.label.map(|label| Label::new(c, label)).transpose()?);
        }

        let remove_label = input
            .remove_label
            .map(|label| Label::new(c, label))
            .transpose()?;
        for label in labels.iter().flatten().chain(&remove_label) {
            children.InsertAtTop(&label.visual)?;
        }
        layers.InsertAtTop(&content)?;

        Ok(Self {
            root,
            frame,
            height: layout.height,
            stroke,
            label_gap: 8.0 * scale,
            indicator_size,
            indicator_y: layout.height - indicator_size - (2.0 * scale).round(),
            frames: [shadow_host.cast()?, glass.cast()?, rim.cast()?],
            outline,
            rim_geometry,
            inner_rim_geometry,
            glint,
            content,
            merge,
            merge_geometry,
            merge_stroke,
            raised: Cell::new(None),
            slots,
            badges,
            attention: (0..layout.slots.len()).map(|_| None).collect(),
            overlays: (0..layout.slots.len()).map(|_| None).collect(),
            attention_theme: theme,
            scale,
            indicators,
            labels,
            remove_label,
        })
    }

    /// Changes only badge visuals: notifications never rebuild the layout or
    /// close an open window picker. Repeated events reuse the same visual.
    /// Shows each app's own overlay icon, as its taskbar button would. Only
    /// these visuals change; an unchanged image keeps its visual.
    pub fn set_overlays(
        &mut self,
        gfx: &Gfx,
        images: impl Iterator<Item = Option<Rc<IconImage>>>,
    ) -> Result<()> {
        for (slot, image) in self.overlays.iter_mut().zip(images) {
            let same = match (slot.as_ref(), image.as_ref()) {
                (Some((old, _)), Some(new)) => Rc::ptr_eq(old, new),
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            if let Some((_, visual)) = slot.take() {
                self.content.Children()?.Remove(&visual)?;
            }
            if let Some(image) = image {
                let c = &gfx.compositor;
                let brush = c.CreateSurfaceBrushWithSurface(&gfx.image_surface(&image)?)?;
                brush.SetStretch(CompositionStretch::Uniform)?;
                let visual = c.CreateSpriteVisual()?;
                visual.SetBrush(&brush)?;
                self.content.Children()?.InsertAtTop(&visual)?;
                *slot = Some((image, visual));
            }
        }
        Ok(())
    }

    pub fn set_attention(&mut self, gfx: &Gfx, pending: impl Iterator<Item = bool>) -> Result<()> {
        for (notice, on) in self.attention.iter_mut().zip(pending) {
            if on && notice.is_none() {
                let badge = AttentionBadge::new(gfx, self.attention_theme)?;
                self.content.Children()?.InsertAtTop(&badge.visual)?;
                *notice = Some(badge);
            }
            if let Some(badge) = notice {
                badge.set_visible(on)?;
            }
        }
        Ok(())
    }

    /// - `lifts`: how far each slot rises above its resting position (bounce).
    /// - `snap`: whether to align to pixels. Used at rest for sharpness; fractional
    ///   positions are retained during movement for smooth animation.
    pub fn apply(&self, frame: &Frame<'_>) -> Result<()> {
        let Frame {
            state,
            hovered,
            lifts,
            drag,
            target,
            slide,
            snap,
        } = *frame;
        let fit = |value: f32| if snap { value.round() } else { value };
        let place = |rect: Rect| self.frame.rect(rect);
        let (slide_x, slide_y) = self.frame.outward(slide);
        self.root.SetOffset(v3(slide_x, slide_y))?;
        self.raise(drag.map(|drag| drag.index))?;
        self.place_merge(target.and_then(|index| {
            Some((
                *state.rects.get(index)?,
                lifts.get(index).copied().unwrap_or(0.0),
            ))
        }))?;

        let plate = place(Rect {
            x: fit(state.left),
            y: 0.0,
            width: fit(state.right - state.left),
            height: self.height,
        });
        let size = v2(plate.width, plate.height);
        let offset = v3(plate.x, plate.y);
        for frame in &self.frames {
            frame.SetSize(size)?;
            frame.SetOffset(offset)?;
        }
        self.outline.SetSize(size)?;
        self.rim_geometry
            .SetSize(v2(size.X - self.stroke, size.Y - self.stroke))?;
        if let Some(inner) = &self.inner_rim_geometry {
            let inset = self.stroke * 3.0;
            inner.SetSize(v2((size.X - inset).max(0.0), (size.Y - inset).max(0.0)))?;
        }
        if let Some(glint) = &self.glint {
            if let Some(rect) = hovered.and_then(|index| state.rects.get(index)) {
                let target_rect = place(*rect);
                let glint_size = self.height * 2.2;
                let cx = target_rect.x + target_rect.width / 2.0 - plate.x;
                let cy = target_rect.y + target_rect.height / 2.0 - plate.y;
                let (gx, gy) = match self.frame.edge {
                    DockEdge::Bottom => (cx - glint_size / 2.0, -glint_size * 0.42),
                    DockEdge::Left => (plate.width - glint_size * 0.58, cy - glint_size / 2.0),
                    DockEdge::Right => (-glint_size * 0.42, cy - glint_size / 2.0),
                };
                glint.SetOffset(v3(fit(gx), fit(gy)))?;
                glint.SetOpacity(1.0)?;
            } else {
                glint.SetOpacity(0.0)?;
            }
        }

        for (index, rect) in state.rects.iter().enumerate() {
            let Rect {
                x,
                y,
                width,
                height,
            } = *rect;
            let dragged = drag.filter(|drag| drag.index == index);
            let (x, y) = match dragged.filter(|drag| !drag.ghost) {
                Some(drag) => (drag.center.0 - width / 2.0, drag.center.1 - height / 2.0),
                None => (x, y - lifts.get(index).copied().unwrap_or(0.0)),
            };
            // In dock coordinates up to here; from here on, in the window.
            let dot = place(Rect {
                x: x + (width - self.indicator_size) / 2.0,
                y: self.indicator_y,
                width: self.indicator_size,
                height: self.indicator_size,
            });
            let Rect {
                x,
                y,
                width,
                height,
            } = place(Rect {
                x,
                y,
                width,
                height,
            });

            if let Some(visual) = self.slots.get(index) {
                visual.SetOffset(v3(fit(x), fit(y)))?;
                visual.SetSize(v2(fit(width), fit(height)))?;
                let opacity = match dragged {
                    Some(drag) if drag.removing => 0.55,
                    Some(drag) if drag.ghost => 0.35,
                    _ => 1.0,
                };
                visual.SetOpacity(opacity)?;
            }
            if let Some(Some(badge)) = self.badges.get(index) {
                let side = width * BADGE_SIZE;
                badge.SetSize(v2(fit(side), fit(side)))?;
                badge.SetOffset(v3(
                    fit(x + width - side * BADGE_INSIDE),
                    fit(y + height - side * BADGE_INSIDE),
                ))?;
            }
            if let Some(Some((_, overlay))) = self.overlays.get(index) {
                let badge = overlay_rect(Rect {
                    x,
                    y,
                    width,
                    height,
                });
                overlay.SetSize(v2(fit(badge.width), fit(badge.height)))?;
                overlay.SetOffset(v3(fit(badge.x), fit(badge.y)))?;
                overlay.SetOpacity(if dragged.is_some_and(|drag| drag.ghost) {
                    0.35
                } else if dragged.is_some_and(|drag| drag.removing) {
                    0.55
                } else {
                    1.0
                })?;
            }
            if let Some(Some(notice)) = self.attention.get(index) {
                notice.place(
                    Rect {
                        x,
                        y,
                        width,
                        height,
                    },
                    self.scale,
                    if dragged.is_some_and(|drag| drag.ghost) {
                        0.35
                    } else if dragged.is_some_and(|drag| drag.removing) {
                        0.55
                    } else {
                        1.0
                    },
                    snap,
                )?;
            }
            if let Some(Some(indicator)) = self.indicators.get(index) {
                indicator.SetIsVisible(dragged.is_none())?;
                indicator.SetOffset(v3(fit(dot.x), fit(dot.y)))?;
            }
            if let Some(Some(label)) = self.labels.get(index) {
                let shown = hovered == Some(index) && drag.is_none();
                label.visual.SetIsVisible(shown)?;
                if shown {
                    self.place_label(
                        label,
                        Rect {
                            x,
                            y,
                            width,
                            height,
                        },
                    )?;
                }
            }
            if let (Some(label), Some(drag)) = (&self.remove_label, dragged) {
                label.visual.SetIsVisible(drag.removing)?;
                if drag.removing {
                    self.place_label(
                        label,
                        Rect {
                            x,
                            y,
                            width,
                            height,
                        },
                    )?;
                }
            }
        }
        if drag.is_none()
            && let Some(label) = &self.remove_label
        {
            label.visual.SetIsVisible(false)?;
        }
        Ok(())
    }

    /// Brings the dragged slot above the others, once per drag (the scene
    /// is rebuilt whenever the order changes).
    fn raise(&self, index: Option<usize>) -> Result<()> {
        if index.is_none() || self.raised.get() == index {
            return Ok(());
        }
        if let Some(visual) = index.and_then(|index| self.slots.get(index)) {
            let children = self.content.Children()?;
            children.Remove(visual)?;
            children.InsertAtTop(visual)?;
            if let Some(Some(badge)) = index.and_then(|index| self.badges.get(index)) {
                children.Remove(badge)?;
                children.InsertAtTop(badge)?;
            }
            if let Some(Some(badge)) = index.and_then(|index| self.attention.get(index)) {
                children.Remove(&badge.visual)?;
                children.InsertAtTop(&badge.visual)?;
            }
            if let Some(Some((_, overlay))) = index.and_then(|index| self.overlays.get(index)) {
                children.Remove(overlay)?;
                children.InsertAtTop(overlay)?;
            }
        }
        self.raised.set(index);
        Ok(())
    }

    /// Frames `target` (its rectangle and how far it is lifted), or hides
    /// the frame.
    fn place_merge(&self, target: Option<(Rect, f32)>) -> Result<()> {
        let Some((rect, lift)) = target else {
            self.merge.SetIsVisible(false)?;
            return Ok(());
        };
        let rect = self.frame.rect(Rect {
            y: rect.y - lift,
            ..rect
        });
        let pad = (rect.width * 0.1).round();
        let size = v2(rect.width + 2.0 * pad, rect.height + 2.0 * pad);
        self.merge.SetOffset(v3(rect.x - pad, rect.y - pad))?;
        self.merge.SetSize(size)?;
        self.merge_geometry
            .SetSize(v2(size.X - self.merge_stroke, size.Y - self.merge_stroke))?;
        let radius = size.X * 0.26;
        self.merge_geometry.SetCornerRadius(v2(radius, radius))?;
        self.merge.SetIsVisible(true)?;
        Ok(())
    }

    /// Beside the icon, on the side away from the screen edge: above it on
    /// a bottom dock, to its right on a left dock, to its left on a right one.
    fn place_label(&self, label: &Label, icon: Rect) -> Result<()> {
        let Rect {
            x,
            y,
            width,
            height,
        } = icon;
        let middle = y + (height - label.height) / 2.0;
        let (left, top) = match self.frame.edge {
            DockEdge::Bottom => (
                x + (width - label.width) / 2.0,
                y - self.label_gap - label.height,
            ),
            DockEdge::Left => (x + width + self.label_gap, middle),
            DockEdge::Right => (x - self.label_gap - label.width, middle),
        };
        label.visual.SetOffset(v3(left.round(), top.round()))?;
        Ok(())
    }
}
