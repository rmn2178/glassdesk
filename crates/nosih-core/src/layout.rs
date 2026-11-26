#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Icon,
    Separator,
}

/// Dock dimensions in caller-selected logical or physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub icon: f32,
    pub gap: f32,
    pub padding: f32,
    pub separator_width: f32,
    pub separator_margin: f32,
}

impl Metrics {
    pub const DEFAULT: Self = Self {
        icon: 48.0,
        // Windows icons fill their square; unlike macOS icons, they have no
        // built-in margin, so the dock provides the spacing.
        gap: 12.0,
        padding: 9.0,
        separator_width: 1.0,
        separator_margin: 7.0,
    };

    #[must_use]
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            icon: self.icon * scale,
            gap: self.gap * scale,
            padding: self.padding * scale,
            // Keep the divider at least one pixel wide so it remains visible.
            separator_width: (self.separator_width * scale).max(1.0),
            separator_margin: self.separator_margin * scale,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub kind: ItemKind,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub slots: Vec<Slot>,
}

/// ```
/// use nosih_core::layout::{ItemKind, Metrics, layout};
/// let result = layout(&[ItemKind::Icon, ItemKind::Icon], &Metrics::DEFAULT);
/// assert_eq!(result.width, 9.0 + 48.0 + 12.0 + 48.0 + 9.0);
/// ```
#[must_use]
pub fn layout(items: &[ItemKind], metrics: &Metrics) -> Layout {
    let m = metrics;
    let mut x = m.padding;
    let slots: Vec<Slot> = items
        .iter()
        .map(|&kind| {
            let rect = match kind {
                ItemKind::Icon => Rect {
                    x,
                    y: m.padding,
                    width: m.icon,
                    height: m.icon,
                },
                ItemKind::Separator => Rect {
                    x: x + m.separator_margin,
                    y: m.padding + m.icon * 0.1,
                    width: m.separator_width,
                    height: m.icon * 0.8,
                },
            };
            x += match kind {
                ItemKind::Icon => m.icon,
                ItemKind::Separator => m.separator_width + 2.0 * m.separator_margin,
            } + m.gap;
            Slot { kind, rect }
        })
        .collect();

    let content_end = if slots.is_empty() { x } else { x - m.gap };
    Layout {
        width: content_end + m.padding,
        height: m.icon + 2.0 * m.padding,
        slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Metrics = Metrics::DEFAULT;

    #[test]
    fn layout_should_be_only_padding_when_empty() {
        let result = layout(&[], &M);
        assert_eq!(
            (result.width, result.height),
            (2.0 * M.padding, M.icon + 2.0 * M.padding)
        );
    }

    #[test]
    fn layout_should_not_add_trailing_gap_after_last_icon() {
        let result = layout(&[ItemKind::Icon; 3], &M);
        assert_eq!(result.width, 3.0 * M.icon + 2.0 * M.gap + 2.0 * M.padding);
    }

    #[test]
    fn layout_should_space_icons_by_icon_plus_gap() {
        let result = layout(&[ItemKind::Icon; 2], &M);
        assert_eq!(
            result.slots[1].rect.x - result.slots[0].rect.x,
            M.icon + M.gap
        );
    }

    #[test]
    fn layout_should_center_separator_line_in_its_slot() {
        let result = layout(&[ItemKind::Icon, ItemKind::Separator, ItemKind::Icon], &M);
        let (left, line, right) = (
            result.slots[0].rect,
            result.slots[1].rect,
            result.slots[2].rect,
        );
        assert_eq!(
            line.x - (left.x + left.width),
            right.x - (line.x + line.width)
        );
    }

    #[test]
    fn layout_should_keep_icons_inside_dock_height() {
        let result = layout(&[ItemKind::Icon], &M);
        let rect = result.slots[0].rect;
        assert_eq!(rect.y + rect.height + M.padding, result.height);
    }

    #[test]
    fn scaled_should_keep_separator_at_least_one_pixel() {
        assert_eq!(M.scaled(0.5).separator_width, 1.0);
    }
}
