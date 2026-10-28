//! Pointer hit-testing and scrollbar geometry: pure arithmetic the runtime turns into actions.

use ratatui::layout::Rect;

use crate::common::actions::Command;

/// A pointer position in absolute screen cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hover {
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarTarget {
    Logs,
    Activity,
    Settings,
}

/// Exact geometry of a rendered vertical scrollbar, retained for pointer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarGeometry {
    pub x: u16,
    pub y: u16,
    pub height: u16,
    pub thumb_y: u16,
    pub thumb_height: u16,
    pub max_scroll: usize,
}

impl ScrollbarGeometry {
    pub fn contains(self, column: u16, row: u16) -> bool {
        column == self.x && row >= self.y && row < self.y.saturating_add(self.height)
    }

    pub fn thumb_contains(self, row: u16) -> bool {
        row >= self.thumb_y && row < self.thumb_y.saturating_add(self.thumb_height)
    }

    /// Pointer row to the model's reverse offset (0 is newest), keeping the grab point.
    pub fn scroll_offset_at(self, row: u16, grab_offset: u16) -> usize {
        let travel = self.height.saturating_sub(self.thumb_height);
        if travel == 0 || self.max_scroll == 0 {
            return 0;
        }
        let thumb_top = row
            .saturating_sub(grab_offset)
            .clamp(self.y, self.y.saturating_add(travel));
        let position = (usize::from(thumb_top.saturating_sub(self.y)) * self.max_scroll
            + usize::from(travel) / 2)
            / usize::from(travel);
        self.max_scroll.saturating_sub(position)
    }

    /// Pointer row to an offset counted from the top (0 is the first line), for a list read
    /// downwards instead of from its newest end.
    pub fn top_offset_at(self, row: u16, grab_offset: u16) -> usize {
        self.max_scroll
            .saturating_sub(self.scroll_offset_at(row, grab_offset))
    }
}

/// A scrollbar drag: which bar and where in the thumb it was grabbed, so the thumb does not jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarDrag {
    pub target: ScrollbarTarget,
    pub grab_offset: u16,
}

/// What a click dispatches, named by the same action the keyboard performs so the two never diverge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Command(Command),
    Help,
}

/// An absolute screen rectangle: `(x, y, width, height)`.
pub type ClickArea = (u16, u16, u16, u16);

/// Whether an absolute rectangle holds a cell.
pub fn area_contains(area: ClickArea, column: u16, row: u16) -> bool {
    column >= area.0
        && column < area.0.saturating_add(area.2)
        && row >= area.1
        && row < area.1.saturating_add(area.3)
}

/// The dialog in front: the box that owns the pointer while it is up, the command a press on the
/// dimmed backdrop means, and the controls the dialog itself drew.
#[derive(Debug, Clone)]
pub struct DialogClicks {
    pub area: ClickArea,
    pub dismiss: Command,
    pub targets: Vec<(ClickArea, Click)>,
}

/// The click targets a frame drew, split by who owns the pointer: the frame's own controls, and
/// the dialog in front, which nothing behind may answer while it is up.
#[derive(Debug, Clone, Default)]
pub struct ClickMap {
    frame: Vec<(ClickArea, Click)>,
    dialog: Option<DialogClicks>,
}

impl ClickMap {
    /// Record a control. Before a dialog opens it belongs to the frame; after, to the dialog.
    pub fn push(&mut self, target: (ClickArea, Click)) {
        match self.dialog.as_mut() {
            Some(dialog) => dialog.targets.push(target),
            None => self.frame.push(target),
        }
    }

    /// Begin a new frame: the previous frame's dialog is gone with it.
    pub fn clear(&mut self) {
        self.frame.clear();
        self.dialog = None;
    }

    /// Put a dialog in front. Every cell of the frame becomes its backdrop until it closes.
    pub fn open_dialog(&mut self, area: ClickArea, dismiss: Command) {
        self.dialog = Some(DialogClicks {
            area,
            dismiss,
            targets: Vec::new(),
        });
    }

    pub fn dialog(&self) -> Option<&DialogClicks> {
        self.dialog.as_ref()
    }

    /// The control under a cell, taken from whoever owns the pointer.
    pub fn under(&self, column: u16, row: u16) -> Option<Click> {
        self.owner()
            .iter()
            .find(|(area, _)| area_contains(*area, column, row))
            .map(|(_, click)| *click)
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Every control the frame drew, the frame's own first: for a test that looks one up.
    pub fn iter(&self) -> impl Iterator<Item = &(ClickArea, Click)> {
        self.frame
            .iter()
            .chain(self.dialog.iter().flat_map(|dialog| dialog.targets.iter()))
    }

    /// Whoever the pointer answers to: the dialog when one is up, the frame otherwise.
    fn owner(&self) -> &[(ClickArea, Click)] {
        match &self.dialog {
            Some(dialog) => &dialog.targets,
            None => &self.frame,
        }
    }
}

impl IntoIterator for ClickMap {
    type Item = (ClickArea, Click);
    type IntoIter = std::vec::IntoIter<(ClickArea, Click)>;

    fn into_iter(mut self) -> Self::IntoIter {
        if let Some(dialog) = self.dialog.take() {
            self.frame.extend(dialog.targets);
        }
        self.frame.into_iter()
    }
}

/// Map a scroll offset to thumb position. Offset 0 (the newest line) sits at the bottom of the track.
pub fn scrollbar_position(offset: usize, content: usize, viewport: usize) -> usize {
    let max = content.saturating_sub(viewport);
    max.saturating_sub(offset.min(max))
}

/// The scrollbar geometry a panel draws, or `None` when everything already fits.
pub fn scrollbar_geometry(
    inner: Rect,
    offset: usize,
    content: usize,
    viewport: usize,
) -> Option<ScrollbarGeometry> {
    if viewport == 0 || content <= viewport {
        return None;
    }
    let area = scrollbar_area(inner);
    let track = usize::from(area.height);
    // A card with no row to draw a thumb in has no scrollbar, whatever is scrolled behind it.
    if track == 0 {
        return None;
    }
    let thumb_height =
        ((viewport.saturating_mul(track) + content / 2) / content).clamp(1, track) as u16;
    let max_scroll = content.saturating_sub(viewport);
    let position = scrollbar_position(offset, content, viewport);
    let thumb_start = ((position.saturating_mul(track) + content / 2) / content)
        .min(track.saturating_sub(usize::from(thumb_height))) as u16;
    Some(ScrollbarGeometry {
        x: area.x,
        y: area.y,
        height: area.height,
        thumb_y: area.y.saturating_add(thumb_start),
        thumb_height,
        max_scroll,
    })
}

/// The scrollbar column, kept one cell away from the panel's right border.
pub fn scrollbar_area(inner: Rect) -> Rect {
    Rect {
        x: inner.x + inner.width.saturating_sub(2),
        width: 1,
        ..inner
    }
}

pub fn row_target_at(targets: &[(u16, u16, u16, u16)], column: u16, row: u16) -> Option<usize> {
    targets.iter().position(|(x, y, width, height)| {
        column >= *x
            && column < x.saturating_add(*width)
            && row >= *y
            && row < y.saturating_add(*height)
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_card_with_no_row_for_a_thumb_draws_no_scrollbar() {
        // Two rows of card leave an empty track; clamping a thumb of 1 into 0 rows used to panic.
        let inner = ratatui::layout::Rect::new(0, 0, 10, 0);
        assert!(
            crate::common::hit_targets::scrollbar_geometry(inner, 0, 100, 5).is_none(),
            "no row to draw a thumb in, so no scrollbar"
        );
    }

    #[test]
    fn log_scrollbar_is_anchored_to_the_bottom_when_following() {
        // Offset 0 is the newest line, at the bottom, so the thumb sits at the bottom of the track.
        assert_eq!(
            crate::common::hit_targets::scrollbar_position(0, 100, 10),
            90
        );
        assert_eq!(
            crate::common::hit_targets::scrollbar_position(90, 100, 10),
            0
        );
        assert_eq!(
            crate::common::hit_targets::scrollbar_position(45, 100, 10),
            45
        );
    }
}
