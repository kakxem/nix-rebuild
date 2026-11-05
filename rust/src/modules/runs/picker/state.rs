//! The run selector's state: the runs, the search, the sort order, and the mark.

use std::io;

use anyhow::Context;
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::common::dialog::{self, Answer};
use crate::common::hit_targets::Hover;
use crate::common::records::{Column, Meta, Order};
use crate::common::terminal::ScreenGuard;

use super::view::{Now, PickerChip, PickerGeometry, View, render_picker};

/// What the selector hands back: a run to open, or the lock a confirmed rollback restores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Open(String),
    Rollback(String),
}

/// The loop's answer to a key; the mark and the search stay in the selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerStep {
    Continue,
    /// Open the run recorded under this id.
    Open(String),
    /// Restore this kept lock and rebuild with it.
    Rollback(String),
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
    /// The run whose kept lock the reader is about to put back, while the question is up.
    confirming: Option<Meta>,
    /// The cell the pointer is on, so the question can light the chip under it.
    hover: Option<Hover>,
}

impl Selector {
    pub fn new(runs: Vec<Meta>) -> Self {
        Self {
            runs,
            query: String::new(),
            filtering: false,
            selected: 0,
            sort: None,
            confirming: None,
            hover: None,
        }
    }

    pub fn hover(&self) -> Option<Hover> {
        self.hover
    }

    pub fn mark_hover(&mut self, column: u16, row: u16) {
        self.hover = Some(Hover { column, row });
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

    /// The run behind the confirmation question, while it is up.
    pub fn confirming(&self) -> Option<&Meta> {
        self.confirming.as_ref()
    }

    /// Whether the marked run kept a lock, which is the only case `R` answers.
    pub fn can_roll_back(&self) -> bool {
        self.visible()
            .get(self.selected)
            .is_some_and(|meta| meta.kept.is_some())
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
        // The question owns the keys while it is up: nothing under it can be typed into.
        if self.confirming.is_some() {
            return self.answer_key(key);
        }
        if self.filtering {
            return self.type_key(key);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => PickerStep::Leave,
            KeyCode::Enter => self.open(),
            KeyCode::Char('r' | 'R') => self.ask_to_roll_back(),
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
            PickerChip::Rollback => self.ask_to_roll_back(),
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

    /// Put the question up for the marked run, if it kept a lock to put back.
    fn ask_to_roll_back(&mut self) -> PickerStep {
        let marked = self
            .visible()
            .get(self.selected)
            .map(|meta| (*meta).clone());
        match marked {
            Some(meta) if meta.kept.is_some() => {
                self.confirming = Some(meta);
                PickerStep::Continue
            }
            // A run that kept no lock has nothing to restore; the key does nothing there.
            _ => PickerStep::Continue,
        }
    }

    /// The confirmation question: `y` restores the lock, anything that says no leaves it be.
    fn answer_key(&mut self, key: KeyEvent) -> PickerStep {
        match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.resolve(true),
            KeyCode::Esc | KeyCode::Char('n' | 'N') | KeyCode::Char('q') => self.resolve(false),
            _ => PickerStep::Continue,
        }
    }

    /// The question's answer, from a key or from a click on one of its two chips.
    pub fn resolve(&mut self, revert: bool) -> PickerStep {
        match (revert, self.confirming.take()) {
            (true, Some(meta)) => meta.kept.map_or(PickerStep::Continue, PickerStep::Rollback),
            _ => PickerStep::Continue,
        }
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
    // The pointer is remembered even while the question is up: its chips light up under it.
    if mouse.kind == MouseEventKind::Moved {
        selector.mark_hover(mouse.column, mouse.row);
    }
    // The question is modal: its two answers and the corner take a click, and nothing behind it
    // answers while it is up.
    if selector.confirming().is_some() {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(answers) = geometry.confirm
            && let Some(answer) = dialog::answer_at(answers, mouse.column, mouse.row)
        {
            return selector.resolve(answer == Answer::Yes);
        }
        return PickerStep::Continue;
    }
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
pub fn pick_run(runs: Vec<Meta>) -> anyhow::Result<Option<Pick>> {
    // Read once, not per frame: the row that built what the checkout holds now carries the mark,
    // and the card above the table names it.
    let now = runs
        .first()
        .and_then(|meta| crate::common::records::lock_digest(&meta.flake))
        .map(|digest| {
            let built = runs
                .iter()
                .find(|meta| meta.lock == digest)
                .map(|meta| format!("built by the {} of {}", meta.action, meta.when()))
                .unwrap_or_else(|| "no recorded run built it".to_owned());
            (digest, built)
        });
    let mut selector = Selector::new(runs);
    let _screen = ScreenGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {
        let visible = selector.visible();
        let rollback = selector.can_roll_back();
        let mut drawn = None;
        terminal.draw(|frame| {
            // The other half of the move, read from the store only while the question is up.
            let kept = selector
                .confirming()
                .and_then(|meta| meta.kept.as_deref())
                .and_then(crate::modules::locks::meta)
                .map(|lock| lock.digest);
            let view = View {
                query: selector.query(),
                filtering: selector.filtering(),
                sort: selector.sort(),
                rollback,
                confirming: selector.confirming(),
                kept: kept.as_deref(),
                hover: selector.hover(),
                now: now.as_ref().map(|(digest, built)| Now {
                    digest: digest.as_str(),
                    built: built.as_str(),
                }),
            };
            drawn = Some(render_picker(frame, &visible, selector.selected(), &view));
        })?;
        // Hit tests run against the frame on screen, so the geometry comes from this draw.
        let geometry = drawn.expect("the frame was drawn");
        let step = match event::read()? {
            CrosstermEvent::Key(key) => selector.press(key),
            CrosstermEvent::Mouse(mouse) => picker_mouse(&mut selector, &geometry, mouse),
            _ => PickerStep::Continue,
        };
        match step {
            PickerStep::Open(id) => return Ok(Some(Pick::Open(id))),
            PickerStep::Rollback(lock) => return Ok(Some(Pick::Rollback(lock))),
            PickerStep::Leave => return Ok(None),
            PickerStep::Continue => {}
        }
    }
}
