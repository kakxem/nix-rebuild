//! The lock selector: one row per kept lock, the mark, and the loop that draws it.

use std::io;

use anyhow::Context;
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::common::dialog::{self, Answer, Answers, Dialog};
use crate::common::hit_targets::{ClickMap, Hover};
use crate::common::records;
use crate::common::terminal::ScreenGuard;
use crate::common::theme::{CYAN, MUTED, SHADE};
use crate::common::time::when;
use crate::common::ui::{panel_block, panel_label};

use super::store::{self, COLUMN_GAP, SavedLock};

/// The loop's answer to a key; the mark stays in the selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerStep {
    Continue,
    /// Put this lock back on the checkout and rebuild with it.
    Open(String),
    Leave,
}

/// Held outside the draw loop so the keys can be exercised without a terminal.
pub struct Selector {
    locks: Vec<SavedLock>,
    selected: usize,
    /// The digest the checkout holds now, so the list can mark the row the reader is standing on.
    current: Option<String>,
    /// The row Enter has asked about: nothing is written until the question is answered.
    asking: Option<usize>,
    /// The cell the pointer is on, so the question can light the chip under it.
    hover: Option<Hover>,
}

impl Selector {
    pub fn new(locks: Vec<SavedLock>, current: Option<String>) -> Self {
        Self {
            locks,
            selected: 0,
            current,
            asking: None,
            hover: None,
        }
    }

    pub fn hover(&self) -> Option<Hover> {
        self.hover
    }

    pub fn mark_hover(&mut self, column: u16, row: u16) {
        self.hover = Some(Hover { column, row });
    }

    pub fn locks(&self) -> &[SavedLock] {
        &self.locks
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn asking(&self) -> Option<usize> {
        self.asking
    }

    pub fn is_current(&self, lock: &SavedLock) -> bool {
        !lock.digest.is_empty() && self.current.as_deref() == Some(lock.digest.as_str())
    }

    /// Apply a key: the arrows move, Enter asks, Esc leaves. There is no search here: a store
    /// this size is read, not filtered.
    pub fn press(&mut self, key: KeyEvent) -> PickerStep {
        if key.kind != KeyEventKind::Press {
            return PickerStep::Continue;
        }
        // Ctrl+C is a key like any other once the terminal is raw.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return PickerStep::Leave;
        }
        if self.asking.is_some() {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.resolve(true),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => self.resolve(false),
                _ => PickerStep::Continue,
            };
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => PickerStep::Leave,
            KeyCode::Enter => self.ask(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_mark(-1);
                PickerStep::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_mark(1);
                PickerStep::Continue
            }
            KeyCode::Home => {
                self.selected = 0;
                PickerStep::Continue
            }
            KeyCode::End => {
                self.selected = self.locks.len().saturating_sub(1);
                PickerStep::Continue
            }
            _ => PickerStep::Continue,
        }
    }

    pub fn mark_row(&mut self, row: usize) {
        self.selected = row.min(self.locks.len().saturating_sub(1));
    }

    pub fn move_mark_by(&mut self, rows: i64) {
        self.move_mark(rows);
    }

    /// Asks about the lock under the mark; leaves when there is nothing to ask about.
    fn ask(&mut self) -> PickerStep {
        if self.locks.get(self.selected).is_none() {
            return PickerStep::Leave;
        }
        self.asking = Some(self.selected);
        PickerStep::Continue
    }

    /// The confirmed answer: this lock is the one that goes back, and the frame starts the run.
    fn put_back(&mut self, row: usize) -> PickerStep {
        self.asking = None;
        match self.locks.get(row) {
            Some(lock) => PickerStep::Open(lock.id.clone()),
            None => PickerStep::Continue,
        }
    }

    /// The question's answer, from a key or from a click on one of its two chips.
    pub fn resolve(&mut self, put_back: bool) -> PickerStep {
        let row = self.asking.take();
        match (put_back, row) {
            (true, Some(row)) => self.put_back(row),
            _ => PickerStep::Continue,
        }
    }

    fn move_mark(&mut self, delta: i64) {
        let last = self.locks.len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + delta).clamp(0, last) as usize;
    }
}

/// Where the selector drew the rows, in cells: the pointer hit-tests the same range.
#[derive(Debug, Clone, Copy)]
pub struct PickerGeometry {
    pub first_row: u16,
    pub rows: usize,
    pub offset: usize,
    pub total: usize,
    /// The question's answers, while it is up: a click answers them like a key does.
    pub confirm: Option<dialog::AnswerAreas>,
}

impl PickerGeometry {
    pub fn row_at(&self, row: u16) -> Option<usize> {
        if row < self.first_row {
            return None;
        }
        let drawn = (row - self.first_row) as usize;
        if drawn >= self.rows {
            return None;
        }
        let index = self.offset + drawn;
        (index < self.total).then_some(index)
    }
}

/// Draw the selector: the state card on top, the table card under it, and the marked lock.
pub fn render_picker(frame: &mut ratatui::Frame<'_>, selector: &Selector) -> PickerGeometry {
    // Two cards stacked the way the rebuild frame stacks them: the state is a card of its own,
    // never a card inside a card.
    let cards = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(NOW_ROWS),
        ratatui::layout::Constraint::Min(0),
    ])
    .split(frame.area());
    state_card(frame, cards[0], selector);

    let legend = "[↑↓  Choose] [Enter  Put one back] [Esc  Leave] ";
    let block = panel_block(
        panel_label("Saved locks"),
        CYAN,
        Some(Line::from(Span::styled(legend, Style::default().fg(MUTED))).right_aligned()),
    );
    let inner = block.inner(cards[1]);
    frame.render_widget(block, cards[1]);

    // One row of air above the headings, then the rows, with one blank kept at the bottom: the
    // padding every card in the rebuild frame keeps.
    let rows = inner.height.saturating_sub(3) as usize;
    let body = ratatui::layout::Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let offset = picker_offset(selector.selected(), selector.locks().len(), rows);
    let mut lines = vec![heading_line()];
    for (index, lock) in selector.locks().iter().enumerate().skip(offset).take(rows) {
        lines.push(lock_row(
            lock,
            index == selector.selected(),
            selector.is_current(lock),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), body);
    let confirm = render_confirm(frame, selector);

    PickerGeometry {
        first_row: body.y + 1,
        rows,
        offset,
        total: selector.locks().len(),
        confirm,
    }
}

/// Rows the state card takes: its two borders and one line of content.
const NOW_ROWS: u16 = 3;

/// The state card, laid out like the rebuild's own cards: the lock the checkout holds now, read
/// from the file itself, and which kept lock that is. The card says where the reader is standing;
/// the marked row in the table says which one.
fn state_card(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, selector: &Selector) {
    let block = panel_block(panel_label("Now"), MUTED, None);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // One cell of air, so the line does not touch the border.
    let text = ratatui::layout::Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    frame.render_widget(Paragraph::new(Line::from(state_spans(selector))), text);
}

/// What the card says: the digest in the colour the table gives it, then which row holds it.
fn state_spans(selector: &Selector) -> Vec<Span<'static>> {
    let Some(digest) = selector.current() else {
        return vec![Span::styled(
            "the checkout has no flake.lock to read",
            Style::default().fg(MUTED),
        )];
    };
    let head = records::head(digest, store::LOCK_COLUMN);
    let note = match selector
        .locks()
        .iter()
        .find(|lock| selector.is_current(lock))
    {
        Some(lock) => format!(
            "the lock kept by the {} of {}",
            lock.reason,
            when(lock.saved_at)
        ),
        None => "no saved lock holds it".to_owned(),
    };
    vec![
        Span::styled(head, Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  ·  {note}"), Style::default().fg(MUTED)),
    ]
}

fn heading_line() -> Line<'static> {
    let headings = store::heading_cells();
    let count = headings.len();
    let mut spans = vec![Span::raw(" ".repeat(MARKER))];
    for (index, cell) in headings.into_iter().enumerate() {
        let text = if index + 1 < count {
            format!("{cell}{COLUMN_GAP}")
        } else {
            cell
        };
        spans.push(Span::styled(
            text,
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// One row: the table's cells, shaded rather than recoloured when it is the marked one, and a
/// bullet instead of the mark when this lock is the one in the checkout.
fn lock_row(lock: &SavedLock, selected: bool, current: bool) -> Line<'static> {
    let shade = selected.then(|| Style::default().bg(SHADE));
    let marker = match shade {
        Some(shade) => Style::default()
            .fg(CYAN)
            .add_modifier(Modifier::BOLD)
            .patch(shade),
        None => Style::default().fg(CYAN),
    };
    let gutter = if selected {
        " ❯ "
    } else if current {
        " • "
    } else {
        "   "
    };
    let mut spans = vec![Span::styled(gutter.to_owned(), marker)];
    let cells = store::cells(lock);
    let count = cells.len();
    for (index, cell) in cells.into_iter().enumerate() {
        let text = if index + 1 < count {
            format!("{cell}{COLUMN_GAP}")
        } else {
            cell
        };
        spans.push(Span::styled(
            text,
            Style::default().fg(if index == 2 { Color::Reset } else { MUTED }),
        ));
    }
    match shade {
        Some(shade) => Line::from(
            spans
                .into_iter()
                .map(|span| span.patch_style(shade))
                .collect::<Vec<_>>(),
        ),
        None => Line::from(spans),
    }
}

/// The two answers the question offers, the same ones `runs` offers for the same move.
const NO_LABEL: &str = "[N  Keep the lock]";
const YES_LABEL: &str = "[Y  Revert and rebuild]";

/// The question asked before the checkout is written: the move the answer makes and its two keys.
/// It is the app's `Dialog`, so it looks and clicks like every other question.
fn render_confirm(
    frame: &mut ratatui::Frame<'_>,
    selector: &Selector,
) -> Option<dialog::AnswerAreas> {
    let lock = selector
        .asking()
        .and_then(|row| selector.locks().get(row))?;
    let arriving = records::head(&lock.digest, store::LOCK_COLUMN);
    let move_line = match selector.current() {
        Some(current) if current == lock.digest => {
            format!("{arriving} is already what the checkout holds.")
        }
        Some(current) => format!(
            "{} → {arriving}",
            records::head(current, store::LOCK_COLUMN)
        ),
        None => format!("→ {arriving}"),
    };
    Dialog::confirm(
        "Rollback",
        format!("Revert the lock of {}?", when(lock.saved_at)),
    )
    .detail(move_line)
    .ask(Answers::new(NO_LABEL, YES_LABEL))
    .render(frame, selector.hover(), &mut ClickMap::default())
    .answers
}

/// The first row the selector shows, so the chosen one stays in view.
fn picker_offset(selected: usize, total: usize, rows: usize) -> usize {
    if rows == 0 || total <= rows {
        return 0;
    }
    selected.saturating_sub(rows / 2).min(total - rows)
}

/// The marker's three cells, counted so the heading lines up with the rows.
const MARKER: usize = 3;

/// Rows the wheel moves, as the runs' selector does.
const WHEEL_ROWS: i64 = 3;

/// Crossterm reports the pointer in the same zero-based cells a frame lays out in.
pub fn lock_mouse(
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
    if selector.asking().is_some() {
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
            // A row is the button: the lock under the pointer is asked about like a key would.
            if let Some(row) = geometry.row_at(mouse.row) {
                selector.mark_row(row);
                return selector.ask();
            }
            PickerStep::Continue
        }
        _ => PickerStep::Continue,
    }
}

/// Ask which kept lock to put back. `None` means the user left without choosing, which is not
/// a failed run.
pub fn pick_lock(locks: Vec<SavedLock>, current: Option<String>) -> anyhow::Result<Option<String>> {
    let mut selector = Selector::new(locks, current);
    let _screen = ScreenGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {
        let mut drawn = None;
        terminal.draw(|frame| drawn = Some(render_picker(frame, &selector)))?;
        // Hit tests run against the frame on screen, so the geometry comes from this draw.
        let geometry = drawn.expect("the frame was drawn");
        let step = match event::read()? {
            CrosstermEvent::Key(key) => selector.press(key),
            CrosstermEvent::Mouse(mouse) => lock_mouse(&mut selector, &geometry, mouse),
            _ => PickerStep::Continue,
        };
        match step {
            PickerStep::Open(id) => return Ok(Some(id)),
            PickerStep::Leave => return Ok(None),
            PickerStep::Continue => {}
        }
    }
}

/// Draw the selector to a string the way the tests read it.
#[cfg(test)]
pub fn picker_to_string(
    locks: &[SavedLock],
    selected: usize,
    current: Option<&str>,
    width: u16,
    height: u16,
) -> String {
    picker_screen(locks, selected, current, false, width, height).0
}

/// The same frame with the question on top, which is what Enter draws before anything is written.
#[cfg(test)]
pub fn picker_to_question_string(
    locks: &[SavedLock],
    selected: usize,
    current: Option<&str>,
    width: u16,
    height: u16,
) -> String {
    picker_screen(locks, selected, current, true, width, height).0
}

/// Where the question drew its answers, so a test can click them.
#[cfg(test)]
pub fn picker_to_question_geometry(
    locks: &[SavedLock],
    selected: usize,
    current: Option<&str>,
    width: u16,
    height: u16,
) -> PickerGeometry {
    picker_screen(locks, selected, current, true, width, height).1
}

#[cfg(test)]
fn picker_screen(
    locks: &[SavedLock],
    selected: usize,
    current: Option<&str>,
    asking: bool,
    width: u16,
    height: u16,
) -> (String, PickerGeometry) {
    use ratatui::backend::TestBackend;

    let mut selector = Selector::new(locks.to_vec(), current.map(str::to_owned));
    selector.selected = selected;
    if asking {
        selector.asking = Some(selected);
    }
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut geometry = None;
    terminal
        .draw(|frame| {
            geometry = Some(render_picker(frame, &selector));
        })
        .expect("render");
    (
        crate::common::ui::buffer_to_string(terminal.backend().buffer()),
        geometry.expect("the frame was drawn"),
    )
}
