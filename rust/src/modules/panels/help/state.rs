//! The help overlay's state: the selection, the `/` search, and where the list landed on screen.

use crate::common::actions::Command;
use crate::common::hit_targets::Hover;

use crate::common::ui::HelpEntry;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub index: usize,
    pub query: String,
    pub filtering: bool,
    /// The absolute rectangle of the help list, for row hit-testing.
    pub area: Option<(u16, u16, u16, u16)>,
    /// Index of the first help row currently drawn.
    pub offset: usize,
    /// The rows every slice contributed, in the order the composition root concatenated them.
    pub entries: Vec<HelpEntry>,
}

impl State {
    pub fn with_entries(entries: Vec<HelpEntry>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }

    pub fn help_row_at(&self, column: u16, row: u16) -> Option<usize> {
        let (x, y, width, height) = self.area?;
        if column < x || column >= x.saturating_add(width) {
            return None;
        }
        if row < y || row >= y.saturating_add(height) {
            return None;
        }
        let index = self.offset + (row - y) as usize;
        (index < self.help_entries().count()).then_some(index)
    }

    pub fn help_hovered_row(&self, hover: Option<Hover>) -> Option<usize> {
        let hover = hover?;
        self.help_row_at(hover.column, hover.row)
    }

    /// The help rows currently visible after the `/` query is applied.
    pub fn help_entries(&self) -> impl Iterator<Item = HelpEntry> + '_ {
        let query = self.query.as_str();
        self.entries
            .iter()
            .copied()
            .filter(move |entry| entry.matches(query))
    }

    /// The selected row's command, if it has one; the dialog closes so the effect is visible.
    /// Rows without an action are documentation only, so Enter just dismisses the list.
    pub fn selected_help_command(&self) -> Option<Command> {
        self.help_entries()
            .nth(self.index)
            .map(|entry| entry.enter_command())
    }

    pub fn move_help_selection(&mut self, delta: i64) {
        let len = self.help_entries().count() as i64;
        if len == 0 {
            self.index = 0;
            return;
        }
        self.index = (self.index as i64 + delta).clamp(0, len - 1) as usize;
    }

    pub fn clear_help_query(&mut self) {
        self.query.clear();
        self.filtering = false;
        self.index = 0;
    }
}
