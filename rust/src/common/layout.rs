//! Layout geometry: panel bodies, scrollbars and the helpers the views centre on.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::common::hit_targets::scrollbar_position;
use crate::common::theme::{GREEN, MUTED};

/// The panel's area inside its border.
pub fn panel_body(panel: Rect) -> Rect {
    Rect {
        x: panel.x.saturating_add(1),
        y: panel.y.saturating_add(1),
        width: panel.width.saturating_sub(2),
        height: panel.height.saturating_sub(2),
    }
}

/// One row of air above and below a card's content, on top of the columns `PANEL_PAD` leaves
/// on each side.
pub const PANEL_PAD_Y: u16 = 1;

/// Strip the vertical padding from an area that is already inside the border.
pub fn padded_rows(area: Rect) -> Rect {
    Rect {
        y: area.y.saturating_add(PANEL_PAD_Y),
        height: area.height.saturating_sub(2 * PANEL_PAD_Y),
        ..area
    }
}

/// The rows a card has for content once its border and padding are taken.
pub fn content_rows(panel: Rect) -> u16 {
    padded_rows(panel_body(panel)).height
}

/// Shrink a panel's inner area by the scrollbar column, if any, plus the padding on each side.
pub fn reserve_scrollbar_column(inner: Rect, needed: bool) -> Rect {
    let right = if needed { PANEL_PAD + 1 } else { PANEL_PAD };
    Rect {
        x: inner.x + PANEL_PAD,
        width: inner.width.saturating_sub(PANEL_PAD + right),
        ..inner
    }
}

/// Columns of air between a card's border and its content. More than one because a Nerd Font
/// glyph is drawn wider than the cell it occupies, so a single column reads as no gap at all.
const PANEL_PAD: u16 = 3;

pub fn panel_scrollbar(
    _offset: usize,
    content: usize,
    viewport: usize,
) -> Option<Scrollbar<'static>> {
    if viewport == 0 || content <= viewport {
        return None;
    }
    Some(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_style(Style::default().fg(MUTED).add_modifier(Modifier::DIM))
            .thumb_style(Style::default().fg(GREEN)),
    )
}

pub fn scrollbar_state(offset: usize, content: usize, viewport: usize) -> ScrollbarState {
    let positions = content.saturating_sub(viewport).saturating_add(1);
    ScrollbarState::new(positions)
        .position(scrollbar_position(offset, content, viewport))
        .viewport_content_length(viewport)
}

pub fn centered_block(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Rect::new(
        area.x,
        area.y + area.height.saturating_sub(height) / 2,
        area.width,
        height,
    )
}

pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_block_sits_on_the_true_vertical_center() {
        let area = ratatui::layout::Rect::new(0, 0, 20, 9);
        assert_eq!(centered_block(area, 2).y, 3);
        assert_eq!(centered_block(area, 2).height, 2);
        assert_eq!(centered_block(area, 9).y, 0);
    }
}
