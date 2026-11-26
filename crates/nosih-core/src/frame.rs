//! Where the dock sits on the screen, and the one mapping between the dock's
//! own coordinates and the screen's.
//!
//! [`layout`](crate::layout::layout) and [`magnify`](crate::magnify::magnify)
//! work in *dock coordinates*: `u` runs along the dock (their `x`), `v`
//! across it (their `y`), with `v = thickness` on the side of the screen
//! edge and icons growing toward smaller `v`. Everything the dock places on
//! screen, and every pointer position it reads, goes through a [`Frame`]:
//! no other code needs to know which edge the dock is on.
//!
//! The dock can sit on the bottom, left or right edge, as on macOS. Not on
//! the top: that edge belongs to the menu bar.
//!
//! On the sides the first item is at the top, as on macOS. Icons are square
//! and the mapping only moves boxes, never rotates their content, so nothing
//! has to be counter-rotated to stay upright.

use crate::layout::Rect;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DockEdge {
    #[default]
    Bottom,
    Left,
    Right,
}

impl DockEdge {
    /// The dock runs from top to bottom.
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// The dock's coordinates placed in a target space (a window or the screen):
/// `origin` is the top-left corner of the dock's resting box there, and
/// `thickness` its depth across (the layout's height).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub edge: DockEdge,
    pub origin: (f32, f32),
    pub thickness: f32,
}

impl Frame {
    #[must_use]
    pub const fn new(edge: DockEdge, origin: (f32, f32), thickness: f32) -> Self {
        Self {
            edge,
            origin,
            thickness,
        }
    }

    /// The target-space point of dock coordinates `(u, v)`.
    #[must_use]
    pub fn point(&self, u: f32, v: f32) -> (f32, f32) {
        let (ox, oy) = self.origin;
        match self.edge {
            DockEdge::Bottom => (ox + u, oy + v),
            DockEdge::Left => (ox + self.thickness - v, oy + u),
            DockEdge::Right => (ox + v, oy + u),
        }
    }

    /// The target-space box of a box in dock coordinates.
    #[must_use]
    pub fn rect(&self, rect: Rect) -> Rect {
        let (ox, oy) = self.origin;
        match self.edge {
            DockEdge::Bottom => Rect {
                x: ox + rect.x,
                y: oy + rect.y,
                ..rect
            },
            DockEdge::Left => Rect {
                x: ox + self.thickness - (rect.y + rect.height),
                y: oy + rect.x,
                width: rect.height,
                height: rect.width,
            },
            DockEdge::Right => Rect {
                x: ox + rect.y,
                y: oy + rect.x,
                width: rect.height,
                height: rect.width,
            },
        }
    }

    /// Dock coordinates `(u, v)` of a target-space point: the inverse of
    /// [`point`](Self::point), for pointer input.
    #[must_use]
    pub fn to_dock(&self, x: f32, y: f32) -> (f32, f32) {
        let (ox, oy) = self.origin;
        match self.edge {
            DockEdge::Bottom => (x - ox, y - oy),
            DockEdge::Left => (y - oy, self.thickness - (x - ox)),
            DockEdge::Right => (y - oy, x - ox),
        }
    }

    /// A move of `distance` away from the screen edge (a bounce, a label's
    /// gap), as a target-space offset.
    #[must_use]
    pub fn inward(&self, distance: f32) -> (f32, f32) {
        match self.edge {
            DockEdge::Bottom => (0.0, -distance),
            DockEdge::Left => (distance, 0.0),
            DockEdge::Right => (-distance, 0.0),
        }
    }

    /// A move of `distance` toward the screen edge, as a target-space offset
    /// (the auto-hidden dock sliding out of view).
    #[must_use]
    pub fn outward(&self, distance: f32) -> (f32, f32) {
        let (x, y) = self.inward(distance);
        (-x, -y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDGES: [DockEdge; 3] = [DockEdge::Bottom, DockEdge::Left, DockEdge::Right];

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn bottom_is_the_identity_shifted_by_the_origin() {
        let frame = Frame::new(DockEdge::Bottom, (100.0, 900.0), 66.0);
        assert_eq!(frame.point(10.0, 9.0), (110.0, 909.0));
        assert_eq!(
            frame.rect(rect(10.0, 9.0, 48.0, 48.0)),
            rect(110.0, 909.0, 48.0, 48.0)
        );
        assert_eq!(frame.inward(5.0), (0.0, -5.0));
    }

    #[test]
    fn points_go_back_to_where_they_came_from() {
        for edge in EDGES {
            let frame = Frame::new(edge, (40.0, 300.0), 66.0);
            for (u, v) in [(0.0, 0.0), (123.5, 9.0), (500.0, -40.0), (12.0, 66.0)] {
                let (x, y) = frame.point(u, v);
                let (back_u, back_v) = frame.to_dock(x, y);
                assert!(
                    (back_u - u).abs() < 1e-4 && (back_v - v).abs() < 1e-4,
                    "{edge:?}"
                );
            }
        }
    }

    #[test]
    fn side_docks_run_top_to_bottom_with_the_first_item_at_the_top() {
        for edge in [DockEdge::Left, DockEdge::Right] {
            let frame = Frame::new(edge, (0.0, 200.0), 66.0);
            let first = frame.rect(rect(9.0, 9.0, 48.0, 48.0));
            let second = frame.rect(rect(69.0, 9.0, 48.0, 48.0));
            assert!(first.y < second.y, "{edge:?}");
            assert_eq!((first.width, first.height), (48.0, 48.0));
        }
    }

    #[test]
    fn the_screen_edge_side_and_growth_point_the_right_way() {
        // Dock coordinate v = thickness is the screen-edge side; icons grow
        // toward smaller v, so into the screen.
        let left = Frame::new(DockEdge::Left, (0.0, 0.0), 66.0);
        assert_eq!(left.point(0.0, 66.0).0, 0.0);
        assert!(left.point(0.0, -20.0).0 > 66.0);
        assert!(left.inward(1.0).0 > 0.0);

        let right = Frame::new(DockEdge::Right, (1854.0, 0.0), 66.0);
        assert_eq!(right.point(0.0, 66.0).0, 1920.0);
        assert!(right.point(0.0, -20.0).0 < 1854.0);
        assert!(right.inward(1.0).0 < 0.0);
    }

    #[test]
    fn a_magnified_icon_keeps_its_base_on_the_edge_side() {
        // A 72 px icon grown from a 48 px one, base still at v = 57.
        let grown = rect(0.0, 57.0 - 72.0, 72.0, 72.0);
        let left = Frame::new(DockEdge::Left, (0.0, 0.0), 66.0).rect(grown);
        assert_eq!(left.x, 66.0 - 57.0);
        assert_eq!(left.width, 72.0);
        let right = Frame::new(DockEdge::Right, (0.0, 0.0), 66.0).rect(grown);
        assert_eq!(right.x + right.width, 57.0);
    }
}
