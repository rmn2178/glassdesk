//! Continuous model: the dock behaves like an elastic band stretching around
//! the cursor. Local stretch at distance `d` is `1 + a·f(d/R)`, where `f` is
//! the inverted *smoothstep* curve. Since `f` is a polynomial, accumulated
//! displacement has a closed form, so icons are positioned independently
//! without jumps or overlap.

use crate::layout::{ItemKind, Layout, Rect};

/// Magnification parameters, in the same units as [`Layout`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnifier {
    /// Distance from the cursor beyond which an icon no longer grows.
    pub range: f32,
    /// Scale of the icon directly under the cursor (1.0 = no magnification).
    pub max_scale: f32,
}

impl Magnifier {
    #[must_use]
    pub fn max_growth(&self) -> f32 {
        // 2·R·F(1), where F(1) = 1/2.
        (self.max_scale - 1.0) * self.range
    }
}

/// `f(d) = 1 − 3d² + 2d³`: 1 under the cursor, 0 from `d = 1` onward.
fn falloff(d: f32) -> f32 {
    let d = d.abs().min(1.0);
    1.0 - 3.0 * d * d + 2.0 * d * d * d
}

/// Integral of `falloff` from 0 to `u`, odd and clamped at ±1/2.
fn falloff_integral(u: f32) -> f32 {
    let d = u.abs().min(1.0);
    (d - d * d * d + d * d * d * d / 2.0).copysign(u)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Magnified {
    /// Left glass edge; expansion extends outward from zero.
    pub left: f32,
    /// Right glass edge; expansion extends outward from the resting width.
    pub right: f32,
    pub rects: Vec<Rect>,
}

impl Magnified {
    /// Index of the icon under coordinate `x`, including half of each adjacent gap.
    #[must_use]
    pub fn icon_at(&self, layout: &Layout, x: f32, gap: f32) -> Option<usize> {
        self.rects
            .iter()
            .zip(&layout.slots)
            .position(|(rect, slot)| {
                slot.kind == ItemKind::Icon
                    && x >= rect.x - gap / 2.0
                    && x < rect.x + rect.width + gap / 2.0
            })
    }
}

/// - `cursor`: horizontal cursor position relative to the resting dock's left edge.
/// - `amount`: 0.0 = resting, 1.0 = full effect (animated on entry and exit).
///
/// Icons grow from their base; dividers only move. Both dock ends expand equally,
/// keeping the dock centered.
#[must_use]
pub fn magnify(layout: &Layout, cursor: f32, amount: f32, magnifier: &Magnifier) -> Magnified {
    let strength = (magnifier.max_scale - 1.0) * amount;
    let range = magnifier.range.max(f32::EPSILON);
    let shift_of = |x: f32| strength * range * falloff_integral((x - cursor) / range);

    let (shift_left, shift_right) = (shift_of(0.0), shift_of(layout.width));
    let centering = -(shift_left + shift_right) / 2.0;

    let rects = layout
        .slots
        .iter()
        .map(|slot| {
            let rest = slot.rect;
            let rest_center = rest.x + rest.width / 2.0;
            let center = rest_center + shift_of(rest_center) + centering;
            let scale = match slot.kind {
                ItemKind::Icon => 1.0 + strength * falloff((rest_center - cursor) / range),
                ItemKind::Separator => 1.0,
            };
            let (width, height) = (rest.width * scale, rest.height * scale);
            Rect {
                x: center - width / 2.0,
                y: rest.y + rest.height - height,
                width,
                height,
            }
        })
        .collect();

    Magnified {
        left: shift_left + centering,
        right: layout.width + shift_right + centering,
        rects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Metrics, layout};

    const M: Metrics = Metrics::DEFAULT;
    const MAGNIFIER: Magnifier = Magnifier {
        range: 210.0,
        max_scale: 1.6,
    };

    fn dock(icons: usize) -> Layout {
        layout(&vec![ItemKind::Icon; icons], &M)
    }

    fn center(rect: &Rect) -> f32 {
        rect.x + rect.width / 2.0
    }

    #[test]
    fn magnify_should_return_rest_layout_when_amount_is_zero() {
        let rest = dock(8);
        let result = magnify(&rest, 100.0, 0.0, &MAGNIFIER);
        let expected: Vec<Rect> = rest.slots.iter().map(|slot| slot.rect).collect();
        assert_eq!(result.rects, expected);
    }

    #[test]
    fn magnify_should_reach_max_scale_on_icon_under_cursor() {
        let rest = dock(8);
        let cursor = center(&rest.slots[3].rect);
        let result = magnify(&rest, cursor, 1.0, &MAGNIFIER);
        assert!((result.rects[3].width - M.icon * 1.6).abs() < 1e-3);
    }

    #[test]
    fn magnify_should_leave_icons_beyond_range_at_rest_size() {
        let rest = dock(12);
        let result = magnify(&rest, center(&rest.slots[0].rect), 1.0, &MAGNIFIER);
        assert_eq!(result.rects[11].width, M.icon);
    }

    #[test]
    fn magnify_should_keep_icon_bases_on_the_same_line() {
        let rest = dock(8);
        let result = magnify(&rest, 150.0, 1.0, &MAGNIFIER);
        let base = M.padding + M.icon;
        assert!(
            result
                .rects
                .iter()
                .all(|r| (r.y + r.height - base).abs() < 1e-3)
        );
    }

    #[test]
    fn magnify_should_never_overlap_neighbouring_icons() {
        let rest = dock(12);
        let overlaps = (0..=rest.width as usize).step_by(7).any(|cursor| {
            let result = magnify(&rest, cursor as f32, 1.0, &MAGNIFIER);
            result
                .rects
                .windows(2)
                .any(|pair| pair[0].x + pair[0].width > pair[1].x)
        });
        assert!(!overlaps);
    }

    #[test]
    fn magnify_should_grow_both_edges_equally() {
        let rest = dock(12);
        let result = magnify(&rest, 137.0, 1.0, &MAGNIFIER);
        assert!((-result.left - (result.right - rest.width)).abs() < 1e-3);
    }

    #[test]
    fn magnify_should_keep_width_constant_while_cursor_is_far_from_edges() {
        let rest = dock(12);
        let width = |cursor: f32| {
            let result = magnify(&rest, cursor, 1.0, &MAGNIFIER);
            result.right - result.left
        };
        let middle = rest.width / 2.0;
        assert!((width(middle) - width(middle + 40.0)).abs() < 1e-3);
    }

    #[test]
    fn max_growth_should_match_width_gain_with_cursor_in_the_middle() {
        let rest = dock(12);
        let result = magnify(&rest, rest.width / 2.0, 1.0, &MAGNIFIER);
        let gain = result.right - result.left - rest.width;
        assert!((gain - MAGNIFIER.max_growth()).abs() < 1e-3);
    }

    #[test]
    fn icon_at_should_find_the_magnified_icon_under_cursor() {
        let rest = dock(8);
        let cursor = center(&rest.slots[5].rect);
        let result = magnify(&rest, cursor, 1.0, &MAGNIFIER);
        let under = center(&result.rects[5]);
        assert_eq!(result.icon_at(&rest, under, M.gap), Some(5));
    }

    #[test]
    fn icon_at_should_ignore_separators() {
        let rest = layout(&[ItemKind::Icon, ItemKind::Separator, ItemKind::Icon], &M);
        let result = magnify(&rest, 0.0, 0.0, &MAGNIFIER);
        let on_line = center(&result.rects[1]);
        assert_eq!(result.icon_at(&rest, on_line, 0.0), None);
    }
}
