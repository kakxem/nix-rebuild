//! The Settings dialog's state: the rows the slices contributed, the selection, and the
//! file's copy the dialog edits. A change is a default for the next start, not for this frame.

use crate::common::config::{SettingEntry, Settings};
use crate::common::hit_targets::ScrollbarGeometry;

/// One line of the card: a section title, the setting under it, or the blank line that
/// separates one section from the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Section(&'static str),
    Entry(usize),
    Gap,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    /// Index into `entries`, never into `lines`.
    pub row: usize,
    /// First line drawn; the selection is kept inside the viewport.
    pub offset: usize,
    /// A pointer put the view here, so the next render leaves it instead of chasing the selection.
    pub offset_pinned: bool,
    /// The absolute rectangle of the list, for hit-testing.
    pub area: Option<(u16, u16, u16, u16)>,
    /// The file's settings: the card edits these, so a change is a default for the next start.
    pub config: Settings,
    pub entries: Vec<SettingEntry>,
    /// `-`/`+` hit areas of the number rows, with the step each applies.
    pub buttons: Vec<((u16, u16, u16, u16), i64)>,
    /// The bar painted last frame, for track clicks and drag.
    pub scrollbar: Option<ScrollbarGeometry>,
    /// The recorded-runs snapshot the read-only rows print; the runtime refreshes it while the card is up.
    pub recorded_runs: usize,
    pub recorded_bytes: u64,
}

impl State {
    pub fn with_entries(entries: Vec<SettingEntry>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }

    /// The card's lines, in order: a blank line before every section but the first, then its
    /// title, then its settings.
    pub fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::with_capacity(self.entries.len() + 6);
        let mut section = "";
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.section != section {
                if !section.is_empty() {
                    lines.push(Line::Gap);
                }
                section = entry.section;
                lines.push(Line::Section(section));
            }
            lines.push(Line::Entry(index));
        }
        lines
    }

    pub fn selected(&self) -> Option<SettingEntry> {
        self.entries.get(self.row).copied()
    }

    /// Move the selection, stepping over rows that cannot be changed, and stop at either end.
    pub fn move_selection(&mut self, delta: i64) {
        let step = delta.signum();
        if step == 0 || self.entries.is_empty() {
            return;
        }
        for _ in 0..delta.unsigned_abs() {
            let Some(next) = self.next_selectable(step) else {
                break;
            };
            self.row = next;
        }
        // A key moves the selection, so the view follows it again after a pointer left it behind.
        self.offset_pinned = false;
    }

    /// The next row the arrows may land on in that direction, or `None` at the end of the list.
    fn next_selectable(&self, step: i64) -> Option<usize> {
        let mut index = self.row as i64 + step;
        loop {
            let entry = usize::try_from(index)
                .ok()
                .and_then(|index| self.entries.get(index))?;
            if entry.is_selectable() {
                return usize::try_from(index).ok();
            }
            index += step;
        }
    }

    /// The first row the arrows may land on.
    pub fn select_first(&mut self) {
        if let Some(index) = self.entries.iter().position(SettingEntry::is_selectable) {
            self.row = index;
            self.offset_pinned = false;
        }
    }

    /// The last row the arrows may land on.
    pub fn select_last(&mut self) {
        if let Some(index) = self.entries.iter().rposition(SettingEntry::is_selectable) {
            self.row = index;
            self.offset_pinned = false;
        }
    }

    /// Keep the selected row inside a viewport of `viewport` lines.
    pub fn set_offset(&mut self, viewport: usize) {
        let viewport = viewport.max(1);
        self.offset = self.offset.min(self.lines().len().saturating_sub(viewport));
        let Some(line) = self.line_of(self.row) else {
            return;
        };
        if self.offset_pinned {
            return;
        }
        if line < self.offset {
            // Keep the line above the selection too: that is the section title the list belongs to.
            self.offset = line.saturating_sub(1);
        } else if line >= self.offset + viewport {
            self.offset = line + 1 - viewport;
        }
        self.offset = self.offset.min(self.lines().len().saturating_sub(viewport));
    }

    /// Put the first drawn line where the card's bar was clicked, leaving the selection be.
    pub fn set_scroll(&mut self, offset: usize) {
        let viewport = self
            .area
            .map(|(_, _, _, height)| height as usize)
            .unwrap_or(1);
        self.offset = offset.min(self.lines().len().saturating_sub(viewport.max(1)));
        self.offset_pinned = true;
    }

    pub fn line_of(&self, index: usize) -> Option<usize> {
        self.lines()
            .iter()
            .position(|line| *line == Line::Entry(index))
    }

    /// The setting under a screen cell, if the cell is on a setting and not on a section title.
    pub fn row_at(&self, column: u16, row: u16) -> Option<usize> {
        let (x, y, width, height) = self.area?;
        if column < x || column >= x.saturating_add(width) {
            return None;
        }
        if row < y || row >= y.saturating_add(height) {
            return None;
        }
        match self.lines().get(self.offset + (row - y) as usize) {
            Some(Line::Entry(index)) if self.entries[*index].is_selectable() => Some(*index),
            _ => None,
        }
    }

    /// The step a `-`/`+` control under the cell applies.
    pub fn button_at(&self, column: u16, row: u16) -> Option<i64> {
        self.buttons
            .iter()
            .find(|(area, _)| contains(*area, column, row))
            .map(|(_, delta)| *delta)
    }

    /// Whether the row at `index` is a number, drawn with its step controls.
    pub fn shows_buttons(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.is_number())
    }
}

fn contains(area: (u16, u16, u16, u16), column: u16, row: u16) -> bool {
    let (x, y, width, height) = area;
    column >= x && column < x.saturating_add(width) && row >= y && row < y.saturating_add(height)
}
