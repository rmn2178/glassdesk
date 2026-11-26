//! Layout math, hit testing, and state management for window preview cards and their close buttons.

use crate::layout::Rect;

/// Minimum physical hit area for interactive controls (20px standard at 100% scale).
pub const MIN_CLOSE_HIT_TARGET: f32 = 20.0;
/// Nominal diameter of the visible close circle at scale 1.0.
pub const CLOSE_BUTTON_DIAMETER: f32 = 18.0;
/// Corner radius used by the preview card's rounded clip.
pub const PREVIEW_CARD_CORNER_RADIUS: f32 = 12.0;
/// Keeps the close button inside the preview card's rounded-corner clip.
pub const CLOSE_BUTTON_INSET: f32 = PREVIEW_CARD_CORNER_RADIUS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardHit {
    None,
    Card(usize),
    Close(usize),
}

/// Geometry of a preview card's close button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloseButtonBounds {
    /// Visual circle bounds (for rendering).
    pub visual: Rect,
    /// Hit target bounds (expanded to meet accessibility / touch guidelines).
    pub hit: Rect,
}

/// Computes the close button's visual and hit rectangles for a given preview card.
#[must_use]
pub fn close_button_bounds(card: Rect, scale: f32) -> CloseButtonBounds {
    let scale = scale.max(0.5);
    let visual_size = (CLOSE_BUTTON_DIAMETER * scale).round();
    let inset = (CLOSE_BUTTON_INSET * scale).ceil();

    let visual = Rect {
        x: card.x + inset,
        y: card.y + inset,
        width: visual_size,
        height: visual_size,
    };

    let hit_size = (MIN_CLOSE_HIT_TARGET * scale).max(visual_size);
    let offset = (hit_size - visual_size) / 2.0;
    let hit = Rect {
        x: visual.x - offset,
        y: visual.y - offset,
        width: hit_size,
        height: hit_size,
    };

    CloseButtonBounds { visual, hit }
}

/// Checks whether `point` is inside `rect`.
#[must_use]
pub fn contains(rect: Rect, point: Point) -> bool {
    point.x >= rect.x
        && point.x < rect.x + rect.width
        && point.y >= rect.y
        && point.y < rect.y + rect.height
}

/// Hit-tests a list of preview card rects against a point. Close button hit
/// targets take precedence over card bodies.
#[must_use]
pub fn hit_test_cards(cards: &[Rect], scale: f32, point: Point) -> CardHit {
    for (i, card) in cards.iter().enumerate() {
        let bounds = close_button_bounds(*card, scale);
        if contains(bounds.hit, point) {
            return CardHit::Close(i);
        }
    }
    for (i, card) in cards.iter().enumerate() {
        if contains(*card, point) {
            return CardHit::Card(i);
        }
    }
    CardHit::None
}

/// Animation state for card dismissal when closing a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClosingCard {
    pub index: usize,
    /// Progress from 0.0 (just clicked) to 1.0 (fully faded/shrunk and removed).
    pub progress: f32,
}

impl ClosingCard {
    #[must_use]
    pub fn new(index: usize) -> Self {
        Self {
            index,
            progress: 0.0,
        }
    }

    /// Advances the closing animation progress with elapsed seconds and total duration.
    pub fn advance(&mut self, elapsed_secs: f32, duration_secs: f32) -> bool {
        if duration_secs <= 0.0 {
            self.progress = 1.0;
            return true;
        }
        self.progress = (self.progress + elapsed_secs / duration_secs).min(1.0);
        self.progress >= 1.0
    }

    /// Card scale factor: 1.0 at start, shrinking to 0.0 at completion.
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        (1.0 - self.progress).max(0.0)
    }

    /// Opacity factor: 1.0 at start, fading to 0.0 at completion.
    #[must_use]
    pub fn opacity_factor(&self) -> f32 {
        (1.0 - self.progress).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_button_hit_target_meets_minimum_at_all_scales() {
        let card = Rect {
            x: 100.0,
            y: 200.0,
            width: 216.0,
            height: 164.0,
        };
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let bounds = close_button_bounds(card, scale);
            assert!(bounds.hit.width >= MIN_CLOSE_HIT_TARGET * scale);
            assert!(bounds.hit.height >= MIN_CLOSE_HIT_TARGET * scale);
            assert!(bounds.visual.x >= card.x);
            assert!(bounds.visual.y >= card.y);
            assert!(bounds.visual.x + bounds.visual.width <= card.x + card.width);
        }
    }

    #[test]
    fn close_button_visual_stays_inside_rounded_corner_clip_at_all_scales() {
        let card = Rect {
            x: 100.0,
            y: 200.0,
            width: 216.0,
            height: 164.0,
        };
        for scale in [0.75, 1.0, 1.1, 1.25, 1.5, 2.0] {
            let bounds = close_button_bounds(card, scale);
            let safe_inset = PREVIEW_CARD_CORNER_RADIUS * scale;
            assert!(bounds.visual.x - card.x >= safe_inset);
            assert!(bounds.visual.y - card.y >= safe_inset);
        }
    }

    #[test]
    fn hit_test_prioritizes_close_button_over_card_body() {
        let card = Rect {
            x: 50.0,
            y: 50.0,
            width: 200.0,
            height: 150.0,
        };
        let cards = [card];
        let bounds = close_button_bounds(card, 1.0);

        // Point inside close button
        let close_pt = Point {
            x: bounds.hit.x + bounds.hit.width / 2.0,
            y: bounds.hit.y + bounds.hit.height / 2.0,
        };
        assert_eq!(hit_test_cards(&cards, 1.0, close_pt), CardHit::Close(0));

        // Point inside card but outside close button
        let body_pt = Point {
            x: card.x + card.width / 2.0,
            y: card.y + card.height / 2.0,
        };
        assert_eq!(hit_test_cards(&cards, 1.0, body_pt), CardHit::Card(0));

        // Point outside card
        let outside_pt = Point {
            x: card.x - 10.0,
            y: card.y - 10.0,
        };
        assert_eq!(hit_test_cards(&cards, 1.0, outside_pt), CardHit::None);
    }

    #[test]
    fn closing_card_state_advances_and_fades() {
        let mut closing = ClosingCard::new(2);
        assert_eq!(closing.index, 2);
        assert_eq!(closing.progress, 0.0);
        assert_eq!(closing.opacity_factor(), 1.0);

        let finished = closing.advance(0.05, 0.1);
        assert!(!finished);
        assert!((closing.progress - 0.5).abs() < 1e-4);
        assert!((closing.opacity_factor() - 0.5).abs() < 1e-4);

        let finished = closing.advance(0.06, 0.1);
        assert!(finished);
        assert_eq!(closing.progress, 1.0);
        assert_eq!(closing.opacity_factor(), 0.0);
    }
}
