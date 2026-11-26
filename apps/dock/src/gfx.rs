//! Graphics device connecting the Windows compositor to Direct2D, allowing
//! icons and masks to be drawn on compositor surfaces.

#[path = "gfx/backdrop.rs"]
mod backdrop;

use anyhow::{Context, Result};
use nosih_core::frame::DockEdge;
use nosih_shell::icons::IconImage;
use windows::{
    Foundation::Size,
    Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat},
    UI::{
        Color,
        Composition::{
            CompositionDrawingSurface, CompositionGraphicsDevice, CompositionSurfaceBrush,
            Compositor,
        },
    },
    Win32::{
        Foundation::{HMODULE, POINT},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT,
                    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
                    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
                D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                D2D1_COLOR_SPACE_SRGB, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
                D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE,
                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
                D2D1_PRIMITIVE_BLEND_COPY, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_ROUNDED_RECT,
                D2D1CreateDevice, ID2D1Device, ID2D1DeviceContext,
            },
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
                D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                DWRITE_READING_DIRECTION_RIGHT_TO_LEFT, DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
                DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
            },
            Dxgi::{Common::DXGI_FORMAT_B8G8R8A8_UNORM, IDXGIDevice},
        },
        System::WinRT::Composition::{ICompositionDrawingSurfaceInterop, ICompositorInterop},
    },
    core::{Interface, w},
};
use windows_numerics::{Matrix3x2, Vector2};

/// Use the first letter, not the app language: media and window titles may
/// use another script. Skip punctuation, numbers and bidi isolation marks.
fn text_reads_rtl(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

fn set_text_direction(format: &IDWriteTextFormat, text: &str) -> Result<()> {
    if text_reads_rtl(text) {
        // SAFETY: The format is live and belongs to the current drawing thread.
        unsafe {
            format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT)?;
            // The brush is cropped to the measured text width. Keep the text
            // at physical x=0 instead of the far end of its 4096px layout box.
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        }
    }
    Ok(())
}

const TRANSPARENT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};
const WHITE: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

/// Shared font glyphs belong to drawing code, which is also used by the
/// standalone diagnostics without a menu bar or application lifecycle.
pub(crate) mod network_glyph {
    pub const WIFI: [char; 4] = ['\u{E872}', '\u{E873}', '\u{E874}', '\u{E701}'];
    pub const WIFI_FULL: char = '\u{E701}';
}

pub struct LabelBrush {
    pub brush: CompositionSurfaceBrush,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub struct Gfx {
    pub compositor: Compositor,
    graphics: CompositionGraphicsDevice,
    d3d: ID3D11Device,
    // Surfaces depend on this device, so it must outlive them.
    _d2d: ID2D1Device,
    dwrite: IDWriteFactory,
    on_error: Option<fn(&anyhow::Error)>,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        let compositor = Compositor::new()?;
        // SAFETY: D3D writes only to the initialized output slot; the returned
        // device is immediately wrapped in an owned COM interface.
        let (d3d, d2d, graphics) = unsafe {
            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let d3d = d3d.context("Direct3D no devolvió un dispositivo")?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let d2d = D2D1CreateDevice(&dxgi, None)?;
            let graphics = compositor
                .cast::<ICompositorInterop>()?
                .CreateGraphicsDevice(&d2d)?;
            (d3d, d2d, graphics)
        };
        Ok(Self {
            compositor,
            graphics,
            d3d,
            _d2d: d2d,
            dwrite: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? },
            on_error: None,
        })
    }

    pub fn with_error_handler(mut self, handler: fn(&anyhow::Error)) -> Self {
        self.on_error = Some(handler);
        self
    }

    /// A read-only query on the UI thread, reusing the watchdog's existing
    /// heartbeat. Detects lost devices even when the dock has nothing to redraw.
    pub fn check_device(&self) -> Result<()> {
        // SAFETY: The owned device is queried only on its creation thread.
        unsafe { self.d3d.GetDeviceRemovedReason()? };
        Ok(())
    }

    /// Creates a `width` × `height` pixel surface, draws on it, and returns it
    /// as a brush for a visual.
    fn draw(
        &self,
        width: u32,
        height: u32,
        paint: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
    ) -> Result<CompositionSurfaceBrush> {
        let surface = self.draw_surface(width, height, paint)?;
        Ok(self.compositor.CreateSurfaceBrushWithSurface(&surface)?)
    }

    fn draw_surface(
        &self,
        width: u32,
        height: u32,
        paint: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
    ) -> Result<CompositionDrawingSurface> {
        self.try_draw_surface(width, height, paint)
            .inspect_err(|error| {
                if let Some(handler) = self.on_error {
                    handler(error);
                }
            })
    }

    fn try_draw_surface(
        &self,
        width: u32,
        height: u32,
        paint: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
    ) -> Result<CompositionDrawingSurface> {
        let surface = self.graphics.CreateDrawingSurface(
            Size {
                Width: width as f32,
                Height: height as f32,
            },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;

        // SAFETY: BeginDraw and EndDraw are paired, and `offset` lives through the call.
        unsafe {
            let mut offset = POINT::default();
            let dc: ID2D1DeviceContext = interop.BeginDraw(None, &mut offset)?;
            // The surface may be part of an atlas, so draw at the specified offset.
            dc.SetDpi(96.0, 96.0);
            dc.SetTransform(&Matrix3x2::translation(offset.x as f32, offset.y as f32));
            dc.Clear(Some(&TRANSPARENT));
            let painted = paint(&dc);
            interop.EndDraw()?;
            painted?;
        }
        Ok(surface)
    }

    pub fn icon_brush(&self, icon: &IconImage, side: u32) -> Result<CompositionSurfaceBrush> {
        self.draw(side, side, |dc| {
            let quality = D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC;
            draw_image(dc, icon, side as f32, side as f32, quality)
        })
    }

    /// A surface holding `image` at its own size, to be shared by many brushes.
    pub fn image_surface(&self, image: &IconImage) -> Result<CompositionDrawingSurface> {
        self.draw_surface(image.width, image.height, |dc| {
            // Drawn 1:1, so there is nothing to interpolate.
            let (width, height) = (image.width as f32, image.height as f32);
            draw_image(
                dc,
                image,
                width,
                height,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            )
        })
    }

    /// Creates a minimal opaque rounded rectangle (side `2·radius + 2`). A
    /// *nine-grid* brush stretches it to any width without repainting or
    /// distorting its corners during magnification.
    pub fn rounded_mask(&self, radius: f32) -> Result<CompositionSurfaceBrush> {
        let side = (2.0 * radius).ceil() + 2.0;
        self.draw(side as u32, side as u32, |dc| {
            let shape = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: side,
                    bottom: side,
                },
                radiusX: radius,
                radiusY: radius,
            };
            // SAFETY: The brush and geometry remain alive during the call.
            unsafe {
                let brush = dc.CreateSolidColorBrush(&WHITE, None)?;
                dc.FillRoundedRectangle(&shape, &brush);
            }
            Ok(())
        })
    }

    /// Opaque island silhouette with curved shoulders at the screen edge.
    pub fn island_mask(
        &self,
        width: f32,
        height: f32,
        scale: f32,
    ) -> Result<CompositionSurfaceBrush> {
        self.draw(width.ceil() as u32, height.ceil() as u32, |dc| {
            let shoulder = (12.0 * scale).min(width / 5.0).min(height / 2.0);
            let corner = (22.0 * scale)
                .min(width / 2.0 - shoulder)
                .min(height - shoulder);
            // SAFETY: The geometries, sink and brush stay alive through the drawing calls.
            unsafe {
                let white = dc.CreateSolidColorBrush(&WHITE, None)?;
                let shape = dc.GetFactory()?.CreatePathGeometry()?;
                let sink = shape.Open()?;
                sink.BeginFigure(Vector2 { X: 0.0, Y: 0.0 }, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(Vector2 { X: width, Y: 0.0 });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: width - shoulder,
                        Y: 0.0,
                    },
                    point2: Vector2 {
                        X: width - shoulder,
                        Y: shoulder * 0.7,
                    },
                    point3: Vector2 {
                        X: width - shoulder,
                        Y: shoulder,
                    },
                });
                sink.AddLine(Vector2 {
                    X: width - shoulder,
                    Y: height - corner,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: width - shoulder,
                        Y: height - corner * 0.35,
                    },
                    point2: Vector2 {
                        X: width - shoulder - corner * 0.35,
                        Y: height,
                    },
                    point3: Vector2 {
                        X: width - shoulder - corner,
                        Y: height,
                    },
                });
                sink.AddLine(Vector2 {
                    X: shoulder + corner,
                    Y: height,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: shoulder + corner * 0.35,
                        Y: height,
                    },
                    point2: Vector2 {
                        X: shoulder,
                        Y: height - corner * 0.35,
                    },
                    point3: Vector2 {
                        X: shoulder,
                        Y: height - corner,
                    },
                });
                sink.AddLine(Vector2 {
                    X: shoulder,
                    Y: shoulder,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: shoulder,
                        Y: shoulder * 0.7,
                    },
                    point2: Vector2 {
                        X: shoulder,
                        Y: 0.0,
                    },
                    point3: Vector2 { X: 0.0, Y: 0.0 },
                });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                dc.FillGeometry(&shape, &white, None);
            }
            Ok(())
        })
    }

    pub fn usage_ring(
        &self,
        used_percent: Option<u8>,
        scale: f32,
        accent: Color,
    ) -> Result<LabelBrush> {
        let side = (18.0 * scale).round();
        let center = side / 2.0;
        let radius = center - 2.0 * scale;
        let stroke = (2.4 * scale).max(1.0);
        let brush = self.draw(side as u32, side as u32, |dc| {
            // SAFETY: Brushes remain alive for the drawing calls.
            unsafe {
                let track = dc.CreateSolidColorBrush(&rgb(0.53, 0.56, 0.6, 0.4), None)?;
                dc.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 {
                            X: center,
                            Y: center,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &track,
                    stroke,
                    None,
                );
                if let Some(percent) = used_percent {
                    let foreground = dc.CreateSolidColorBrush(
                        &rgb(
                            f32::from(accent.R) / 255.0,
                            f32::from(accent.G) / 255.0,
                            f32::from(accent.B) / 255.0,
                            1.0,
                        ),
                        None,
                    )?;
                    let steps = usize::from(percent) * 36 / 100;
                    for index in 0..steps {
                        let start = -std::f32::consts::FRAC_PI_2
                            + index as f32 * std::f32::consts::TAU / 36.0;
                        let end = start + std::f32::consts::TAU / 36.0;
                        let a = Vector2 {
                            X: center + radius * start.cos(),
                            Y: center + radius * start.sin(),
                        };
                        let b = Vector2 {
                            X: center + radius * end.cos(),
                            Y: center + radius * end.sin(),
                        };
                        dc.DrawLine(a, b, &foreground, stroke, None);
                    }
                }
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width: side,
            height: side,
        })
    }

    /// Creates an app-name label: a dark pill (light in light theme) with an
    /// arrow pointing at the icon: down on a bottom dock, toward the screen
    /// edge on a side dock (the label sits beside the icon there).
    pub fn label_brush(
        &self,
        text: &str,
        scale: f32,
        light: bool,
        edge: DockEdge,
    ) -> Result<LabelBrush> {
        let (pad_x, pad_y, arrow) = (11.0 * scale, 5.0 * scale, 6.0 * scale);
        let (fill, ink) = if light {
            (rgb(0.96, 0.96, 0.97, 0.92), rgb(0.0, 0.0, 0.0, 0.88))
        } else {
            (rgb(0.13, 0.13, 0.14, 0.86), rgb(1.0, 1.0, 1.0, 0.95))
        };

        let utf16: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: Strings and structures passed to each call remain alive for its duration.
        let (layout, metrics) = unsafe {
            let format = self.dwrite.CreateTextFormat(
                text_font(),
                None,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0 * scale,
                w!(""),
            )?;
            set_text_direction(&format, text)?;
            let layout = self
                .dwrite
                .CreateTextLayout(&utf16, &format, 4096.0, 256.0)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            (layout, metrics)
        };

        let body_height = (metrics.height + 2.0 * pad_y).ceil();
        let body_width = (metrics.width + 2.0 * pad_x).ceil();
        let (width, height) = if edge.is_vertical() {
            (body_width + arrow, body_height)
        } else {
            (body_width, body_height + arrow)
        };
        // Where the pill starts: after the arrow on a left dock's label.
        let body_left = if edge == DockEdge::Left { arrow } else { 0.0 };
        // The arrow: its base on the pill (1 px into it, so no seam shows)
        // and its tip pointing at the icon.
        let (base_a, base_b, tip) = match edge {
            DockEdge::Bottom => (
                (width / 2.0 - arrow, body_height - 1.0),
                (width / 2.0 + arrow, body_height - 1.0),
                (width / 2.0, height),
            ),
            DockEdge::Left => (
                (arrow + 1.0, height / 2.0 - arrow),
                (arrow + 1.0, height / 2.0 + arrow),
                (0.0, height / 2.0),
            ),
            DockEdge::Right => (
                (body_width - 1.0, height / 2.0 - arrow),
                (body_width - 1.0, height / 2.0 + arrow),
                (width, height / 2.0),
            ),
        };
        let brush = self.draw(width as u32, height as u32, |dc| {
            let body = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: body_left,
                    top: 0.0,
                    right: body_left + body_width,
                    bottom: body_height,
                },
                radiusX: 7.0 * scale,
                radiusY: 7.0 * scale,
            };
            // SAFETY: All created objects remain alive until the end of the block.
            unsafe {
                let fill = dc.CreateSolidColorBrush(&fill, None)?;
                dc.FillRoundedRectangle(&body, &fill);

                let triangle = dc.GetFactory()?.CreatePathGeometry()?;
                let sink = triangle.Open()?;
                let point = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
                sink.BeginFigure(point(base_a), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(point(base_b));
                sink.AddLine(point(tip));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                dc.FillGeometry(&triangle, &fill, None);

                let ink = dc.CreateSolidColorBrush(&ink, None)?;
                dc.DrawTextLayout(
                    Vector2 {
                        X: body_left + pad_x,
                        Y: pad_y,
                    },
                    &layout,
                    &ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// A group's icon: a frosted rounded square with a rim, holding the first
    /// apps' icons in a 2×2 grid, so a group never looks like a single app.
    /// With more than four apps the last cell counts the others. `icons` are
    /// the first apps' images (`None` while one is loading); `count` is how
    /// many apps the group has.
    pub fn group_brush(
        &self,
        side: u32,
        icons: &[Option<&IconImage>],
        count: usize,
        light: bool,
    ) -> Result<CompositionSurfaceBrush> {
        let size = side as f32;
        let inset = size * 0.08;
        let tile = size - 2.0 * inset;
        let radius = tile * 0.225;
        let pad = tile * 0.13;
        let gap = tile * 0.08;
        let cell = (tile - 2.0 * pad - gap) / 2.0;
        let stroke = (size * 0.022).max(1.0);
        let more = if count > 4 {
            Some(self.centered_text(&format!("+{}", count - 3), cell * 0.5, cell)?)
        } else {
            None
        };
        let (fill_color, rim_color, slot_color, ink_color) = if light {
            (
                rgb(1.0, 1.0, 1.0, 0.55),
                rgb(0.0, 0.0, 0.0, 0.22),
                rgb(0.0, 0.0, 0.0, 0.10),
                rgb(0.0, 0.0, 0.0, 0.75),
            )
        } else {
            (
                rgb(0.30, 0.30, 0.33, 0.62),
                rgb(1.0, 1.0, 1.0, 0.34),
                rgb(1.0, 1.0, 1.0, 0.12),
                rgb(1.0, 1.0, 1.0, 0.92),
            )
        };
        self.draw(side, side, |dc| {
            let half = stroke / 2.0;
            // SAFETY: Brushes, the text layout and the images stay alive
            // through the drawing calls.
            unsafe {
                let fill = dc.CreateSolidColorBrush(&fill_color, None)?;
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: inset,
                            top: inset,
                            right: size - inset,
                            bottom: size - inset,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &fill,
                );
                let rim = dc.CreateSolidColorBrush(&rim_color, None)?;
                dc.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: inset + half,
                            top: inset + half,
                            right: size - inset - half,
                            bottom: size - inset - half,
                        },
                        radiusX: radius - half,
                        radiusY: radius - half,
                    },
                    &rim,
                    stroke,
                    None,
                );
                let slot = dc.CreateSolidColorBrush(&slot_color, None)?;
                let ink = dc.CreateSolidColorBrush(&ink_color, None)?;
                for index in 0..count.min(4) {
                    let left = inset + pad + (index % 2) as f32 * (cell + gap);
                    let top = inset + pad + (index / 2) as f32 * (cell + gap);
                    let rect = D2D_RECT_F {
                        left,
                        top,
                        right: left + cell,
                        bottom: top + cell,
                    };
                    let rounded = D2D1_ROUNDED_RECT {
                        rect,
                        radiusX: cell * 0.22,
                        radiusY: cell * 0.22,
                    };
                    if index == 3
                        && let Some(more) = &more
                    {
                        dc.FillRoundedRectangle(&rounded, &slot);
                        dc.DrawTextLayout(
                            Vector2 { X: left, Y: top },
                            more,
                            &ink,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    } else if let Some(Some(icon)) = icons.get(index) {
                        draw_image_in(dc, icon, rect, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC)?;
                    } else {
                        dc.FillRoundedRectangle(&rounded, &slot);
                    }
                }
            }
            Ok(())
        })
    }

    /// One line centered in a `side` × `side` square, to draw inside an icon.
    fn centered_text(&self, text: &str, font_size: f32, side: f32) -> Result<IDWriteTextLayout> {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: The string and the format live through each call.
        unsafe {
            let format = self.dwrite.CreateTextFormat(
                text_font(),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size.max(1.0),
                w!(""),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(self
                .dwrite
                .CreateTextLayout(&utf16, &format, side.max(1.0), side.max(1.0))?)
        }
    }

    /// A name on a rounded pill, cut with an ellipsis to fit `max_width`:
    /// the names beside the apps of an unfolded group.
    pub fn pill_brush(
        &self,
        text: &str,
        scale: f32,
        light: bool,
        max_width: f32,
    ) -> Result<LabelBrush> {
        let (pad_x, pad_y) = (10.0 * scale, 4.0 * scale);
        let (fill_color, ink_color) = if light {
            (rgb(0.96, 0.96, 0.97, 0.92), rgb(0.0, 0.0, 0.0, 0.88))
        } else {
            (rgb(0.13, 0.13, 0.14, 0.86), rgb(1.0, 1.0, 1.0, 0.95))
        };
        let room = (max_width - 2.0 * pad_x).max(1.0);
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: Strings and structures passed to each call remain alive for its duration.
        let (layout, metrics) = unsafe {
            let format = self.dwrite.CreateTextFormat(
                text_font(),
                None,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0 * scale,
                w!(""),
            )?;
            set_text_direction(&format, text)?;
            let layout = self.dwrite.CreateTextLayout(&utf16, &format, room, 256.0)?;
            layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let ellipsis = self.dwrite.CreateEllipsisTrimmingSign(&format)?;
            layout.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    ..Default::default()
                },
                &ellipsis,
            )?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            (layout, metrics)
        };
        let width = (metrics.width.ceil().clamp(1.0, room) + 2.0 * pad_x).ceil();
        let height = (metrics.height + 2.0 * pad_y).ceil();
        let brush = self.draw(width as u32, height as u32, |dc| {
            // SAFETY: The brushes and the layout live through the drawing calls.
            unsafe {
                let fill = dc.CreateSolidColorBrush(&fill_color, None)?;
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: 0.0,
                            top: 0.0,
                            right: width,
                            bottom: height,
                        },
                        radiusX: height / 2.0,
                        radiusY: height / 2.0,
                    },
                    &fill,
                );
                let ink = dc.CreateSolidColorBrush(&ink_color, None)?;
                dc.DrawTextLayout(
                    Vector2 { X: pad_x, Y: pad_y },
                    &layout,
                    &ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// Shared temporary app tile while a shell icon is loading or unavailable.
    pub fn placeholder_brush(&self, side: u32, light: bool) -> Result<CompositionSurfaceBrush> {
        let size = side as f32;
        self.draw(side, side, |dc| {
            let tile = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: size * 0.08,
                    top: size * 0.08,
                    right: size * 0.92,
                    bottom: size * 0.92,
                },
                radiusX: size * 0.18,
                radiusY: size * 0.18,
            };
            let shade = if light { 0.72 } else { 0.36 };
            // SAFETY: Geometry and brushes remain valid for these drawing calls.
            unsafe {
                let fill = dc.CreateSolidColorBrush(&rgb(shade, shade, shade, 1.0), None)?;
                dc.FillRoundedRectangle(&tile, &fill);
                let ink = dc.CreateSolidColorBrush(&rgb(1.0, 1.0, 1.0, 0.7), None)?;
                dc.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: size * 0.27,
                            top: size * 0.28,
                            right: size * 0.73,
                            bottom: size * 0.72,
                        },
                        radiusX: size * 0.04,
                        radiusY: size * 0.04,
                    },
                    &ink,
                    (size * 0.035).max(1.0),
                    None,
                );
            }
            Ok(())
        })
    }

    /// Circular "✕" close button for preview thumbnails and notification banners.
    pub fn close_button_brush(
        &self,
        side: u32,
        hovered: bool,
        light: bool,
    ) -> Result<CompositionSurfaceBrush> {
        let size = side as f32;
        self.draw(side, side, |dc| {
            let center = size / 2.0;
            let radius = (size / 2.0) - 1.0;
            let ellipse = D2D1_ELLIPSE {
                point: Vector2 {
                    X: center,
                    Y: center,
                },
                radiusX: radius,
                radiusY: radius,
            };
            // SAFETY: Direct2D drawing calls with valid brushes.
            unsafe {
                let (bg_color, cross_color) = if hovered {
                    // macOS traffic-light close button red when hovered
                    (rgb(1.0, 0.37, 0.34, 1.0), rgb(1.0, 1.0, 1.0, 0.95))
                } else if light {
                    (rgb(0.0, 0.0, 0.0, 0.22), rgb(1.0, 1.0, 1.0, 0.9))
                } else {
                    (rgb(1.0, 1.0, 1.0, 0.22), rgb(1.0, 1.0, 1.0, 0.9))
                };
                let bg_brush = dc.CreateSolidColorBrush(&bg_color, None)?;
                dc.FillEllipse(&ellipse, &bg_brush);

                let cross_brush = dc.CreateSolidColorBrush(&cross_color, None)?;
                let arm = radius * 0.42;
                let stroke_width = (size * 0.08).max(1.2);
                dc.DrawLine(
                    Vector2 {
                        X: center - arm,
                        Y: center - arm,
                    },
                    Vector2 {
                        X: center + arm,
                        Y: center + arm,
                    },
                    &cross_brush,
                    stroke_width,
                    None,
                );
                dc.DrawLine(
                    Vector2 {
                        X: center - arm,
                        Y: center + arm,
                    },
                    Vector2 {
                        X: center + arm,
                        Y: center - arm,
                    },
                    &cross_brush,
                    stroke_width,
                    None,
                );
            }
            Ok(())
        })
    }

    pub fn start_brush(&self, side: u32) -> Result<CompositionSurfaceBrush> {
        let size = side as f32;
        self.draw(side, side, |dc| {
            // Same margin and corner ratio as a macOS app icon.
            let inset = size * 0.08;
            let tile = D2D_RECT_F {
                left: inset,
                top: inset,
                right: size - inset,
                bottom: size - inset,
            };
            let radius = (size - 2.0 * inset) * 0.225;
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: rgb(0.40, 0.80, 1.0, 1.0),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: rgb(0.04, 0.42, 0.95, 1.0),
                },
            ];
            // SAFETY: Brushes and geometry live for the duration of the drawing calls.
            unsafe {
                let collection = dc.CreateGradientStopCollection(
                    &stops,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                )?;
                let gradient = dc.CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2 {
                            X: 0.0,
                            Y: tile.top,
                        },
                        endPoint: Vector2 {
                            X: 0.0,
                            Y: tile.bottom,
                        },
                    },
                    None,
                    &collection,
                )?;
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: tile,
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &gradient,
                );

                // The Windows logo: four square panes split by a thin gap
                // (0.85 of 18.5 units in its SVG), on whole pixels so the
                // edges stay sharp at every icon size.
                let white = dc.CreateSolidColorBrush(&rgb(1.0, 1.0, 1.0, 0.97), None)?;
                let logo = (size * 0.44).round();
                let gap = (logo * 0.046).round().max(1.0);
                let pane = ((logo - gap) / 2.0).floor();
                let origin = ((size - (2.0 * pane + gap)) / 2.0).round();
                for row in 0..2 {
                    for column in 0..2 {
                        let left = origin + column as f32 * (pane + gap);
                        let top = origin + row as f32 * (pane + gap);
                        dc.FillRectangle(
                            &D2D_RECT_F {
                                left,
                                top,
                                right: left + pane,
                                bottom: top + pane,
                            },
                            &white,
                        );
                    }
                }
            }
            Ok(())
        })
    }

    /// Plain text on a transparent background (menu bar).
    pub fn text_brush(&self, text: &str, scale: f32, bold: bool, ink: Ink) -> Result<LabelBrush> {
        self.text_brush_sized(text, scale, 13.0, bold, ink)
    }

    /// Text at a chosen logical font size for the expanded usage panel.
    pub fn text_brush_sized(
        &self,
        text: &str,
        scale: f32,
        font_size: f32,
        bold: bool,
        ink: Ink,
    ) -> Result<LabelBrush> {
        self.draw_text_brush(text, scale, font_size, bold, ink, None)
    }

    /// Single line, elided to the available physical width.
    pub fn text_brush_fitted(
        &self,
        text: &str,
        scale: f32,
        font_size: f32,
        bold: bool,
        ink: Ink,
        max_width: f32,
    ) -> Result<LabelBrush> {
        self.draw_text_brush(text, scale, font_size, bold, ink, Some(max_width))
    }

    fn draw_text_brush(
        &self,
        text: &str,
        scale: f32,
        font_size: f32,
        bold: bool,
        ink: Ink,
        fitted: Option<f32>,
    ) -> Result<LabelBrush> {
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        if fitted.is_some() {
            for unit in &mut utf16 {
                if matches!(*unit, 9..=13 | 0x85 | 0x2028 | 0x2029) {
                    *unit = u16::from(b' ');
                }
            }
        }
        let max_width = fitted.map_or(4096.0, |width| width.floor().max(1.0));
        let weight = if bold {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        // SAFETY: Strings and structures passed to each call remain alive for its duration.
        let (layout, metrics) = unsafe {
            let format = self.dwrite.CreateTextFormat(
                text_font(),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size * scale,
                w!(""),
            )?;
            set_text_direction(&format, text)?;
            let layout = self
                .dwrite
                .CreateTextLayout(&utf16, &format, max_width, 256.0)?;
            if fitted.is_some() {
                layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let ellipsis = self.dwrite.CreateEllipsisTrimmingSign(&format)?;
                layout.SetTrimming(
                    &DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        ..Default::default()
                    },
                    &ellipsis,
                )?;
            }
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            (layout, metrics)
        };
        let (width, height) = (
            fitted.map_or_else(
                || metrics.width.ceil().max(1.0),
                |_| metrics.width.ceil().clamp(1.0, max_width),
            ),
            metrics.height.ceil().max(1.0),
        );
        let brush = self.draw(width as u32, height as u32, |dc| {
            // SAFETY: The brush and layout live for the drawing call.
            unsafe {
                let ink = dc.CreateSolidColorBrush(&ink.color(), None)?;
                dc.DrawTextLayout(
                    Vector2 { X: 0.0, Y: 0.0 },
                    &layout,
                    &ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// Symbols from the system icon font (see `icon_font`), centered
    /// in a square. Layers are drawn in order with their opacity, so a dim full
    /// Wi-Fi fan can sit behind the bars actually received.
    pub fn glyph_brush(&self, layers: &[(char, f32)], scale: f32, ink: Ink) -> Result<LabelBrush> {
        let side = (18.0 * scale).round();
        // SAFETY: The format lives for the drawing below.
        let format = unsafe {
            let format = self.dwrite.CreateTextFormat(
                icon_font(),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0 * scale,
                w!(""),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format
        };
        let brush = self.draw(side as u32, side as u32, |dc| {
            let area = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: side,
                bottom: side,
            };
            // SAFETY: The brush, format and text live for each drawing call.
            unsafe {
                let ink = dc.CreateSolidColorBrush(&ink.color(), None)?;
                for &(glyph, opacity) in layers {
                    let mut units = [0u16; 2];
                    let text = glyph.encode_utf16(&mut units);
                    ink.SetOpacity(opacity);
                    dc.DrawText(
                        text,
                        &format,
                        &area,
                        &ink,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width: side,
            height: side,
        })
    }

    /// Wi-Fi icon filled to `bars` with faint fan background.
    /// If `has_internet` is false, an exclamation mark is drawn beside the Wi-Fi icon.
    pub fn wifi_brush(
        &self,
        bars: u8,
        has_internet: bool,
        scale: f32,
        ink: Ink,
    ) -> Result<LabelBrush> {
        let icon_side = (18.0 * scale).round();
        let width = if has_internet {
            icon_side
        } else {
            (icon_side + 7.0 * scale).round()
        };
        let height = icon_side;
        // SAFETY: The format lives for the drawing below.
        let format = unsafe {
            let format = self.dwrite.CreateTextFormat(
                icon_font(),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0 * scale,
                w!(""),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format
        };
        let brush = self.draw(width as u32, height as u32, |dc| {
            let icon_area = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: icon_side,
                bottom: height,
            };
            // SAFETY: The brush, format and text live for each drawing call.
            unsafe {
                let ink_brush = dc.CreateSolidColorBrush(&ink.color(), None)?;
                let wifi_glyph = network_glyph::WIFI[usize::from(bars.clamp(1, 4)) - 1];
                for &(glyph, opacity) in &[(network_glyph::WIFI_FULL, 0.3f32), (wifi_glyph, 1.0f32)]
                {
                    let mut units = [0u16; 2];
                    let text = glyph.encode_utf16(&mut units);
                    ink_brush.SetOpacity(opacity);
                    dc.DrawText(
                        text,
                        &format,
                        &icon_area,
                        &ink_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                if !has_internet {
                    let warn_color = rgb(0.98, 0.60, 0.0, 1.0);
                    let warn_brush = dc.CreateSolidColorBrush(&warn_color, None)?;
                    let ex_x = icon_side + 2.5 * scale;
                    let stroke = (1.8 * scale).max(1.0);
                    let bar_top = height * 0.22;
                    let bar_bottom = height * 0.58;
                    dc.DrawLine(
                        Vector2 {
                            X: ex_x,
                            Y: bar_top,
                        },
                        Vector2 {
                            X: ex_x,
                            Y: bar_bottom,
                        },
                        &warn_brush,
                        stroke,
                        None,
                    );
                    let dot_radius = (stroke * 0.55).max(1.0);
                    let dot_y = height * 0.74;
                    dc.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 { X: ex_x, Y: dot_y },
                            radiusX: dot_radius,
                            radiusY: dot_radius,
                        },
                        &warn_brush,
                    );
                }
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// Two switches, one on and one off: the control center glyph.
    pub fn control_center_brush(&self, scale: f32, ink: Ink) -> Result<LabelBrush> {
        let (width, height) = ((19.0 * scale).round(), (16.0 * scale).round());
        let brush = self.draw(width as u32, height as u32, |dc| {
            let pill_height = height * 0.44;
            let radius = pill_height / 2.0;
            let stroke = (1.2 * scale).max(1.0);
            // SAFETY: The brush lives for the drawing calls.
            unsafe {
                let ink = dc.CreateSolidColorBrush(&ink.color(), None)?;
                for (row, knob_right) in [(0.0, true), (height - pill_height, false)] {
                    let pill = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: stroke / 2.0,
                            top: row + stroke / 2.0,
                            right: width - stroke / 2.0,
                            bottom: row + pill_height - stroke / 2.0,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    };
                    dc.DrawRoundedRectangle(&pill, &ink, stroke, None);
                    let center = if knob_right { width - radius } else { radius };
                    dc.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 {
                                X: center,
                                Y: row + radius,
                            },
                            radiusX: radius - stroke * 0.3,
                            radiusY: radius - stroke * 0.3,
                        },
                        &ink,
                    );
                }
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }

    /// Battery outline filled to `percent`, green while charging, orange on saver.
    pub fn battery_brush(
        &self,
        percent: u8,
        charging: bool,
        saver: bool,
        scale: f32,
        ink: Ink,
    ) -> Result<LabelBrush> {
        let (width, height) = ((26.0 * scale).round(), (12.0 * scale).round());
        let brush = self.draw(width as u32, height as u32, |dc| {
            let stroke = (1.0 * scale).max(1.0);
            let cap = 2.0 * scale;
            let body_right = width - cap - stroke;
            let level = if charging {
                rgb(0.20, 0.78, 0.35, 1.0)
            } else if saver {
                rgb(0.98, 0.60, 0.0, 1.0)
            } else if percent <= 20 {
                rgb(1.0, 0.27, 0.23, 1.0)
            } else {
                ink.color()
            };
            // SAFETY: The brushes live for the drawing calls.
            unsafe {
                let outline = dc.CreateSolidColorBrush(&ink.faded(), None)?;
                let fill = dc.CreateSolidColorBrush(&level, None)?;
                dc.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: stroke / 2.0,
                            top: stroke / 2.0,
                            right: body_right,
                            bottom: height - stroke / 2.0,
                        },
                        radiusX: 3.0 * scale,
                        radiusY: 3.0 * scale,
                    },
                    &outline,
                    stroke,
                    None,
                );
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: body_right + stroke,
                            top: height * 0.32,
                            right: width,
                            bottom: height * 0.68,
                        },
                        radiusX: scale,
                        radiusY: scale,
                    },
                    &outline,
                );
                let inset = stroke * 2.0;
                let room = body_right - 2.0 * inset;
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: inset,
                            top: inset,
                            right: inset + room * f32::from(percent.min(100)) / 100.0,
                            bottom: height - inset,
                        },
                        radiusX: 1.5 * scale,
                        radiusY: 1.5 * scale,
                    },
                    &fill,
                );
                if charging {
                    // A bolt over the middle of the body, as on a Mac, with a
                    // clear gap cut around it so it reads over the level.
                    let bolt = dc.GetFactory()?.CreatePathGeometry()?;
                    let sink = bolt.Open()?;
                    let (bolt_width, bolt_height) = (height * 0.62, height * 1.04);
                    let left = (body_right - bolt_width) / 2.0;
                    let top = (height - bolt_height) / 2.0;
                    let at = |x: f32, y: f32| Vector2 {
                        X: left + x * bolt_width,
                        Y: top + y * bolt_height,
                    };
                    sink.BeginFigure(at(0.62, 0.0), D2D1_FIGURE_BEGIN_FILLED);
                    for (x, y) in [
                        (0.08, 0.58),
                        (0.46, 0.58),
                        (0.36, 1.0),
                        (0.92, 0.40),
                        (0.54, 0.40),
                    ] {
                        sink.AddLine(at(x, y));
                    }
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    sink.Close()?;
                    let clear = dc.CreateSolidColorBrush(&TRANSPARENT, None)?;
                    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                    dc.DrawGeometry(&bolt, &clear, 2.0 * stroke, None);
                    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                    let ink = dc.CreateSolidColorBrush(&ink.color(), None)?;
                    dc.FillGeometry(&bolt, &ink, None);
                }
            }
            Ok(())
        })?;
        Ok(LabelBrush {
            brush,
            width,
            height,
        })
    }
}

fn draw_image(
    dc: &ID2D1DeviceContext,
    image: &IconImage,
    width: f32,
    height: f32,
    interpolation: D2D1_INTERPOLATION_MODE,
) -> Result<()> {
    let dest = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: width,
        bottom: height,
    };
    draw_image_in(dc, image, dest, interpolation)
}

/// Draws `image` stretched over `dest`.
fn draw_image_in(
    dc: &ID2D1DeviceContext,
    image: &IconImage,
    dest: D2D_RECT_F,
    interpolation: D2D1_INTERPOLATION_MODE,
) -> Result<()> {
    let properties = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        ..Default::default()
    };
    // SAFETY: `image.bgra` contains `width * height * 4` bytes and the stride is `width * 4`.
    unsafe {
        let bitmap = dc.CreateBitmap(
            D2D_SIZE_U {
                width: image.width,
                height: image.height,
            },
            Some(image.bgra.as_ptr().cast()),
            image.width * 4,
            &properties,
        )?;
        dc.DrawBitmap(&bitmap, Some(&dest), 1.0, interpolation, None, None);
    }
    Ok(())
}

/// The interface font: Segoe UI Variable on Windows 11, Segoe UI on
/// Windows 10, which does not have it.
fn text_font() -> windows::core::PCWSTR {
    if nosih_shell::system::is_windows_11() {
        w!("Segoe UI Variable Text")
    } else {
        w!("Segoe UI")
    }
}

/// The system icon font. Windows 10 has Segoe MDL2 Assets instead of Segoe
/// Fluent Icons; the symbols the menu bar uses share the same code points.
fn icon_font() -> windows::core::PCWSTR {
    if nosih_shell::system::is_windows_11() {
        w!("Segoe Fluent Icons")
    } else {
        w!("Segoe MDL2 Assets")
    }
}

const fn rgb(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::Win32::{
        Foundation::D2DERR_RECREATE_TARGET,
        System::WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        },
    };

    #[test]
    fn labels_follow_the_first_letter_instead_of_the_ui_locale() {
        for text in [
            "العربية",
            "(فارغ)",
            "65% مستخدم",
            "\u{2067}جارٍ التحديث…\u{2069}",
        ] {
            assert!(text_reads_rtl(text), "{text}");
        }
        for text in [
            "Nosih",
            "Claude Code (العربية)",
            "简体中文",
            "हिन्दी",
            "বাংলা",
            "65%",
            "",
        ] {
            assert!(!text_reads_rtl(text), "{text}");
        }
    }

    #[test]
    fn drawing_error_reaches_observer_and_balances_begin_end_draw() -> Result<()> {
        static ERRORS: AtomicUsize = AtomicUsize::new(0);
        let _queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let gfx = Gfx::new()?.with_error_handler(|_| {
            ERRORS.fetch_add(1, Ordering::Relaxed);
        });
        gfx.check_device()?;
        // Synthetic loss at the drawing boundary; the real software device
        // remains usable, allowing us to verify EndDraw ran despite the error.
        let failed = gfx.draw_surface(2, 2, |_| {
            Err(windows::core::Error::from_hresult(D2DERR_RECREATE_TARGET).into())
        });
        assert!(failed.is_err());
        assert_eq!(ERRORS.load(Ordering::Relaxed), 1);
        gfx.draw_surface(2, 2, |_| Ok(()))?;
        gfx.check_device()?;
        Ok(())
    }
}

/// Foreground color of menu bar content, by theme.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Dark content for a light bar.
    Dark,
    /// Light content for a dark bar.
    Light,
}

impl Ink {
    fn color(self) -> D2D1_COLOR_F {
        match self {
            Self::Dark => rgb(0.0, 0.0, 0.0, 0.86),
            Self::Light => rgb(1.0, 1.0, 1.0, 0.94),
        }
    }

    fn faded(self) -> D2D1_COLOR_F {
        match self {
            Self::Dark => rgb(0.0, 0.0, 0.0, 0.5),
            Self::Light => rgb(1.0, 1.0, 1.0, 0.55),
        }
    }
}
