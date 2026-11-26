//! Genie effect: a window is sucked into its dock slot.
//!
//! The window is cut into horizontal strips. Two things happen, overlapping:
//! 1. its side edges bend into two S-curves that run from the window's top
//!    corners to the slot (`bend`);
//! 2. the whole image slides down between those curves and disappears into
//!    the slot (`slide`).
//!
//! A strip's width depends only on where it currently is on screen, so the
//! image narrows as it travels, like liquid through a funnel.

use std::f32::consts::PI;

/// Screen rectangle by its edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Where a source strip is drawn in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// Rows of the source image, as fractions of its height.
    pub source_top: f32,
    pub source_bottom: f32,
    /// `None` once the strip has entered the slot.
    pub target: Option<Edges>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Genie {
    pub window: Edges,
    pub slot: Edges,
}

/// The edges finish bending at this fraction of the animation.
const BEND_END: f32 = 0.5;
/// The image starts sliding at this fraction.
const SLIDE_START: f32 = 0.15;

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Genie {
    /// How far the side edges have bent toward the slot at time `t` (0..=1).
    #[must_use]
    pub fn bend(t: f32) -> f32 {
        smoothstep(t / BEND_END)
    }

    /// How far the image has slid into the slot at time `t` (0..=1).
    #[must_use]
    pub fn slide(t: f32) -> f32 {
        let x = ((t - SLIDE_START) / (1.0 - SLIDE_START)).clamp(0.0, 1.0);
        // Accelerating, like something being pulled in.
        x * x
    }

    /// Left and right edges of the image at screen height `y`.
    fn sides(&self, y: f32, bend: f32) -> (f32, f32) {
        let span = self.slot.top - self.window.top;
        let along = if span > 0.0 {
            ((y - self.window.top) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let curve = (1.0 - (PI * along).cos()) / 2.0 * bend;
        (
            self.window.left + (self.slot.left - self.window.left) * curve,
            self.window.right + (self.slot.right - self.window.right) * curve,
        )
    }

    /// Places `strips.len()` equal strips for time `t` (0 = untouched window,
    /// 1 = fully inside the slot). Reuses the buffer: no allocation per frame.
    pub fn frame(&self, t: f32, strips: &mut [Strip]) {
        let count = strips.len().max(1) as f32;
        let height = self.window.bottom - self.window.top;
        let bend = Self::bend(t);
        let shift = Self::slide(t) * (self.slot.bottom - self.window.top).max(0.0);

        for (index, strip) in strips.iter_mut().enumerate() {
            let source_top = index as f32 / count;
            let source_bottom = (index + 1) as f32 / count;
            let top = self.window.top + source_top * height + shift;
            let bottom = (self.window.top + source_bottom * height + shift).min(self.slot.bottom);
            strip.source_top = source_top;
            strip.source_bottom = source_bottom;
            strip.target = (top < self.slot.bottom).then(|| {
                let (left, right) = self.sides((top + bottom) / 2.0, bend);
                Edges {
                    left,
                    top,
                    right,
                    bottom,
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENIE: Genie = Genie {
        window: Edges {
            left: 200.0,
            top: 100.0,
            right: 1000.0,
            bottom: 600.0,
        },
        slot: Edges {
            left: 1200.0,
            top: 1000.0,
            right: 1260.0,
            bottom: 1060.0,
        },
    };

    fn frame(t: f32) -> Vec<Strip> {
        let mut strips = vec![
            Strip {
                source_top: 0.0,
                source_bottom: 0.0,
                target: None,
            };
            50
        ];
        GENIE.frame(t, &mut strips);
        strips
    }

    fn targets(t: f32) -> Vec<Edges> {
        frame(t).iter().filter_map(|strip| strip.target).collect()
    }

    #[test]
    fn frame_should_show_the_untouched_window_at_the_start() {
        let all = targets(0.0);
        let exact = all
            .iter()
            .all(|edges| edges.left == 200.0 && edges.right == 1000.0);
        assert!(exact && all[0].top == 100.0 && all[49].bottom == 600.0);
    }

    #[test]
    fn frame_should_leave_nothing_outside_the_slot_at_the_end() {
        assert!(targets(1.0).is_empty());
    }

    #[test]
    fn strips_should_stay_contiguous() {
        let gaps = [0.2, 0.5, 0.8].iter().any(|&t| {
            targets(t)
                .windows(2)
                .any(|pair| (pair[0].bottom - pair[1].top).abs() > 1e-3)
        });
        assert!(!gaps);
    }

    #[test]
    fn strips_should_never_be_drawn_below_the_slot() {
        let below = (0..=20).any(|i| {
            targets(i as f32 / 20.0)
                .iter()
                .any(|edges| edges.bottom > 1060.0 || edges.top >= 1060.0)
        });
        assert!(!below);
    }

    #[test]
    fn image_should_narrow_toward_the_slot() {
        let all = targets(0.6);
        let widths: Vec<f32> = all.iter().map(|edges| edges.right - edges.left).collect();
        assert!(widths.windows(2).all(|pair| pair[1] <= pair[0] + 1e-3));
    }

    #[test]
    fn image_should_never_be_narrower_than_the_slot() {
        let narrower = (0..=20).any(|i| {
            targets(i as f32 / 20.0)
                .iter()
                .any(|edges| edges.right - edges.left < 60.0 - 1e-3)
        });
        assert!(!narrower);
    }

    #[test]
    fn top_edge_should_not_move_before_the_slide_starts() {
        assert_eq!(targets(SLIDE_START)[0].top, 100.0);
    }

    #[test]
    fn strips_should_cover_the_whole_source() {
        let strips = frame(0.3);
        assert!(strips[0].source_top == 0.0 && strips[49].source_bottom == 1.0);
    }
}
