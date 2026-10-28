//! The run selector's state: the runs, the search, the sort order, and the mark.

use std::io;

use anyhow::Context;
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::common::records::{Column, Meta, Order};
use crate::common::terminal::ScreenGuard;

use super::view::{PickerChip, PickerGeometry, render_picker};

/// The loop's answer to a key; the mark and the search stay in the selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerStep {
    Continue,
    /// Open the run recorded under this id.
    Open(String),
    Leave,
}

/// Held outside the draw loop so the keys can be exercised without a terminal.
pub struct Selector {
    runs: Vec<Meta>,
    query: String,
    filtering: bool,
    selected: usize,
    /// Sort column and direction; `None` is the kept order (newest first).
    sort: Option<(Column, Order)>,
}

impl Selector {
    pub fn new(runs: Vec<Meta>) -> Self {
        Self {
            runs,
            query: String::new(),
            filtering: false,
            selected: 0,
            sort: None,
        }
    }

    /// The runs the search leaves, in display order.
    pub fn visible(&self) -> Vec<&Meta> {
        let mut runs: Vec<&Meta> = self
            .runs
            .iter()
            .filter(|meta| meta.matches(&self.query))
            .collect();
        if let Some((column, order)) = self.sort {
            runs.sort_by(|left, right| column.compare(left, right));
            if order == Order::Descending {
                runs.reverse();
            }
        }
        runs
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn filtering(&self) -> bool {
        self.filtering
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn sort(&self) -> Option<(Column, Order)> {
        self.sort
    }

    /// Cycles a column: ascending, then descending, then back to the kept order.
    pub fn sort_by(&mut self, column: Column) {
        self.sort = match self.sort {
            Some((sorted, Order::Ascending)) if sorted == column => {
                Some((column, Order::Descending))
            }
            Some((sorted, Order::Descending)) if sorted == column => None,
            _ => Some((column, Order::Ascending)),
        };
        self.mark(0);
    }

    /// Apply a key. While the search is open the letters are text; Esc clears it before leaving.
    pub fn press(&mut self, key: KeyEvent) -> PickerStep {
        if key.kind != KeyEventKind::Press {
            return PickerStep::Continue;
        }
        // Ctrl+C is a key like any other once the terminal is raw.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return PickerStep::Leave;
        }
        if self.filtering {
            return self.type_key(key);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => PickerStep::Leave,
            KeyCode::Enter => self.open(),
            KeyCode::Char('/') => {
                self.filtering = true;
                PickerStep::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_mark(-1);
                PickerStep::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_mark(1);
                PickerStep::Continue
            }
            KeyCode::Home => {
                self.mark(0);
                PickerStep::Continue
            }
            KeyCode::End => {
                self.mark(usize::MAX);
                PickerStep::Continue
            }
            _ => PickerStep::Continue,
        }
    }

    pub fn press_chip(&mut self, chip: PickerChip) -> PickerStep {
        match chip {
            // The chip names both arrows; a click moves the mark down.
            PickerChip::Choose => {
                self.move_mark(1);
                PickerStep::Continue
            }
            PickerChip::Open => self.open(),
            PickerChip::Search => {
                self.filtering = true;
                PickerStep::Continue
            }
            PickerChip::Clear => {
                self.clear_search();
                PickerStep::Continue
            }
            PickerChip::Leave => PickerStep::Leave,
        }
    }

    pub fn mark_row(&mut self, row: usize) {
        self.mark(row);
    }

    pub fn move_mark_by(&mut self, rows: i64) {
        self.move_mark(rows);
    }

    fn type_key(&mut self, key: KeyEvent) -> PickerStep {
        match key.code {
            KeyCode::Enter => self.open(),
            KeyCode::Esc => {
                self.clear_search();
                PickerStep::Continue
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.mark(0);
                PickerStep::Continue
            }
            KeyCode::Up => {
                self.move_mark(-1);
                PickerStep::Continue
            }
            KeyCode::Down => {
                self.move_mark(1);
                PickerStep::Continue
            }
            KeyCode::Char(letter) => {
                self.query.push(letter);
                self.mark(0);
                PickerStep::Continue
            }
            _ => PickerStep::Continue,
        }
    }

    fn clear_search(&mut self) {
        self.query.clear();
        self.filtering = false;
        self.mark(0);
    }

    /// Opens the run under the mark; leaves when the search left none.
    fn open(&self) -> PickerStep {
        match self.visible().get(self.selected) {
            Some(meta) => PickerStep::Open(meta.id.clone()),
            None => PickerStep::Leave,
        }
    }

    fn mark(&mut self, row: usize) {
        let last = self.visible().len().saturating_sub(1);
        self.selected = row.min(last);
    }

    fn move_mark(&mut self, delta: i64) {
        let last = self.visible().len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + delta).clamp(0, last) as usize;
    }
}

const WHEEL_ROWS: i64 = 3;

/// Crossterm reports the pointer in the same zero-based cells a frame lays out in.
pub fn picker_mouse(
    selector: &mut Selector,
    geometry: &PickerGeometry,
    mouse: MouseEvent,
) -> PickerStep {
    match mouse.kind {
        MouseEventKind::Moved => {
            if let Some(row) = geometry.row_at(mouse.row) {
                selector.mark_row(row);
            }
            PickerStep::Continue
        }
        MouseEventKind::ScrollUp => {
            selector.move_mark_by(-WHEEL_ROWS);
            PickerStep::Continue
        }
        MouseEventKind::ScrollDown => {
            selector.move_mark_by(WHEEL_ROWS);
            PickerStep::Continue
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Chips route first: the title is outside the list, so a key hint must not hit the list.
            if let Some(chip) = geometry.chip_at(mouse.column, mouse.row) {
                return selector.press_chip(chip);
            }
            if let Some(column) = geometry.column_at(mouse.column, mouse.row) {
                selector.sort_by(column);
                return PickerStep::Continue;
            }
            if let Some(row) = geometry.row_at(mouse.row) {
                selector.mark_row(row);
                return selector.open();
            }
            PickerStep::Continue
        }
        _ => PickerStep::Continue,
    }
}

/// Ask which recorded run to open, from the rows the caller reads. `None` means the user
/// left without choosing, which is not a failed run.
pub fn pick_run(runs: Vec<Meta>) -> anyhow::Result<Option<String>> {
    let mut selector = Selector::new(runs);
    let _screen = ScreenGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {
        let visible = selector.visible();
        let mut drawn = None;
        terminal.draw(|frame| {
            drawn = Some(render_picker(
                frame,
                &visible,
                selector.selected(),
                selector.query(),
                selector.filtering(),
                selector.sort(),
            ))
        })?;
        // Hit tests run against the frame on screen, so the geometry comes from this draw.
        let geometry = drawn.expect("the frame was drawn");
        let step = match event::read()? {
            CrosstermEvent::Key(key) => selector.press(key),
            CrosstermEvent::Mouse(mouse) => picker_mouse(&mut selector, &geometry, mouse),
            _ => PickerStep::Continue,
        };
        match step {
            PickerStep::Open(id) => return Ok(Some(id)),
            PickerStep::Leave => return Ok(None),
            PickerStep::Continue => {}
        }
    }
}
