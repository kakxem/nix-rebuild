//! The selector's frame: the run table, its chips, and where they land for hit tests.

use ratatui::Frame;
#[cfg(test)]
use ratatui::Terminal;
#[cfg(test)]
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::common::dialog::{self, Answers, Dialog};
use crate::common::hit_targets::{ClickMap, Hover};
use crate::common::records::{self, Column, Meta, Order};
use crate::common::theme::{CYAN, GREEN, MUTED, RED, SHADE, YELLOW};
use crate::common::ui::{panel_block, panel_label};

/// A clickable key hint in the title; a click presses the key it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerChip {
    Choose,
    Open,
    Rollback,
    Search,
    Clear,
    Leave,
}

pub const PICKER_CHIPS: [(PickerChip, &str); 4] = [
    (PickerChip::Choose, "[↑↓  Choose]"),
    (PickerChip::Open, "[Enter  Open]"),
    (PickerChip::Search, "[/  Search]"),
    (PickerChip::Leave, "[Esc  Leave]"),
];

/// The same legend with `R` in it. The title carries the key when the terminal is wide enough for
/// a fifth chip; below that the footer hint says it instead.
pub const PICKER_ROLLBACK_CHIPS: [(PickerChip, &str); 5] = [
    (PickerChip::Choose, "[↑↓  Choose]"),
    (PickerChip::Open, "[Enter  Open]"),
    (PickerChip::Rollback, "[R  Revert]"),
    (PickerChip::Search, "[/  Search]"),
    (PickerChip::Leave, "[Esc  Leave]"),
];

/// The rollback hint, on the blank row kept under the list, for the widths where the title cannot
/// hold a fifth chip. Clicking it does what `R` does.
pub const ROLLBACK_HINT: &str = " R  Revert this run's lock ";

/// Chips while the search is open, where the letters are text rather than keys.
pub const PICKER_SEARCH_CHIPS: [(PickerChip, &str); 2] = [
    (PickerChip::Open, "[Enter  Open]"),
    (PickerChip::Clear, "[Esc  Clear]"),
];

pub const PICKER_MARKER: u16 = 3;

/// Where the selector drew what, in cells.
/// The renderer fills these ranges and the pointer hit-tests the same ones.
#[derive(Debug, Clone)]
pub struct PickerGeometry {
    /// Title border row, where the chips are.
    pub title_row: u16,
    pub header_row: u16,
    pub first_row: u16,
    /// How many entries fit in the frame.
    pub rows: usize,
    /// The entry drawn first.
    pub offset: usize,
    pub total: usize,
    /// The x range of each column, the gaps around it included.
    pub columns: [(u16, u16); 8],
    pub chips: Vec<(PickerChip, u16, u16)>,
    /// The rollback hint on the footer row: its row and x range, while the mark keeps a lock.
    pub rollback: Option<(u16, u16, u16)>,
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

    pub fn column_at(&self, column: u16, row: u16) -> Option<Column> {
        if row != self.header_row {
            return None;
        }
        self.columns
            .iter()
            .position(|(start, end)| column >= *start && column < *end)
            .map(|index| Column::ALL[index])
    }

    pub fn chip_at(&self, column: u16, row: u16) -> Option<PickerChip> {
        if let Some((hint_row, start, end)) = self.rollback
            && row == hint_row
            && column >= start
            && column < end
        {
            return Some(PickerChip::Rollback);
        }
        if row != self.title_row {
            return None;
        }
        self.chips
            .iter()
            .find(|(_, start, end)| column >= *start && column < *end)
            .map(|(chip, _, _)| *chip)
    }
}

/// Everything the frame reads besides the rows and the mark: the filter, the order, and the two
/// facts that depend on what the checkout holds now.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub query: &'a str,
    pub filtering: bool,
    pub sort: Option<(Column, Order)>,
    /// The marked run keeps a lock, so `R` has something to put back.
    pub rollback: bool,
    /// The run the question is up for: the box is drawn over the list, in the same pass.
    pub confirming: Option<&'a Meta>,
    /// The digest of the lock that run kept, when the store still has it: the other half of the move.
    pub kept: Option<&'a str>,
    /// What the checkout holds now, and which run built it.
    pub now: Option<Now<'a>>,
    /// The cell the pointer is on, so the question can light the chip under it.
    pub hover: Option<Hover>,
}

/// The digest the checkout holds now, read once when the picker opens, and the run that built it.
#[derive(Debug, Clone, Copy)]
pub struct Now<'a> {
    pub digest: &'a str,
    /// `built by the update of 31 Oct 21:58`, or that no recorded run did.
    pub built: &'a str,
}

/// Draw the selector; the returned geometry is what the pointer is matched against.
pub fn render_picker(
    frame: &mut Frame<'_>,
    runs: &[&Meta],
    selected: usize,
    view: &View<'_>,
) -> PickerGeometry {
    let View {
        query,
        filtering,
        sort,
        rollback,
        confirming,
        kept,
        now,
        hover,
    } = *view;
    // Two cards stacked the way the rebuild frame stacks them: the state is a card of its own,
    // never a card inside a card.
    let cards = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(NOW_ROWS),
        ratatui::layout::Constraint::Min(0),
    ])
    .split(frame.area());
    state_card(frame, cards[0], now);

    let label = if filtering || !query.is_empty() {
        format!("Recorded runs  /{query}")
    } else {
        "Recorded runs".to_owned()
    };
    let chips = chips_for(cards[1].width, &label, filtering, rollback);
    let legend = format!("{} ", legend_of(chips));
    let block = panel_block(
        panel_label(&label),
        CYAN,
        Some(Line::from(Span::styled(legend.clone(), Style::default().fg(MUTED))).right_aligned()),
    );
    let inner = block.inner(cards[1]);
    frame.render_widget(block, cards[1]);

    // The runs lose three rows: the air above the headings, the headings, and the blank kept at
    // the bottom, the padding every card in the rebuild frame keeps.
    let rows = inner.height.saturating_sub(3) as usize;
    let body = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let offset = picker_offset(selected, runs.len(), rows);
    let mut lines = vec![heading_line(sort)];
    for (index, meta) in runs.iter().enumerate().skip(offset).take(rows) {
        let current = now.is_some_and(|now| !meta.lock.is_empty() && now.digest == meta.lock);
        lines.push(picker_row(meta, index == selected, current));
    }
    if runs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no run matches the search   Esc clears it",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), body);

    // `R` says itself in the title where the label leaves room; the footer carries it otherwise.
    let carried = chips.iter().any(|(chip, _)| *chip == PickerChip::Rollback);
    let hint = rollback_hint(frame, inner, rollback && !carried);
    // The question goes over the list, so the row it asks about stays readable behind it.
    let confirm = confirming.and_then(|meta| render_confirm(frame, meta, kept, hover));
    PickerGeometry {
        title_row: cards[1].y,
        header_row: body.y,
        first_row: body.y + 1,
        rows,
        offset,
        total: runs.len(),
        columns: column_ranges(inner),
        chips: chip_ranges(cards[1], &legend, chips),
        rollback: hint,
        confirm,
    }
}

/// Draw the rollback hint on the blank row under the list, when the mark keeps a lock, and say
/// where it landed so a click lands on it like a key.
fn rollback_hint(frame: &mut Frame<'_>, inner: Rect, rollback: bool) -> Option<(u16, u16, u16)> {
    if !rollback || inner.height < 2 {
        return None;
    }
    let width = (ROLLBACK_HINT.chars().count() as u16).min(inner.width);
    let area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            ROLLBACK_HINT,
            Style::default().fg(YELLOW),
        ))),
        area,
    );
    Some((area.y, area.x, area.x + width))
}

fn legend_of(chips: &[(PickerChip, &str)]) -> String {
    chips
        .iter()
        .map(|(_, label)| *label)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The legend the title can hold: `R` joins the four keys only when the label leaves it room.
fn chips_for(
    width: u16,
    label: &str,
    filtering: bool,
    rollback: bool,
) -> &'static [(PickerChip, &'static str)] {
    if filtering {
        return &PICKER_SEARCH_CHIPS;
    }
    if !rollback {
        return &PICKER_CHIPS;
    }
    // Inside the two borders: the label with its rule and its trailing space, a cell of air, and
    // the legend as the title draws it, with one trailing space of its own.
    let legend = legend_of(&PICKER_ROLLBACK_CHIPS).chars().count() as u16;
    let needed = label.chars().count() as u16 + 3 + 1 + legend;
    if needed <= width.saturating_sub(2) {
        &PICKER_ROLLBACK_CHIPS
    } else {
        &PICKER_CHIPS
    }
}

/// Where each chip lands: counted back from the right end of the title row where the legend draws.
fn chip_ranges(
    area: Rect,
    legend: &str,
    chips: &[(PickerChip, &str)],
) -> Vec<(PickerChip, u16, u16)> {
    let end = area.right().saturating_sub(1);
    let start = end.saturating_sub(legend.chars().count() as u16);
    let mut at = start;
    let mut ranges = Vec::with_capacity(chips.len());
    for (index, (chip, label)) in chips.iter().enumerate() {
        let width = label.chars().count() as u16;
        ranges.push((*chip, at, at + width));
        if index + 1 == chips.len() {
            break;
        }
        // One cell between two chips, the same as the legend's own spacing.
        at += width + 1;
    }
    ranges
}

/// Where each column lands across the frame, gaps included.
/// A click anywhere over a heading works; the last column runs to the right edge.
fn column_ranges(inner: Rect) -> [(u16, u16); 8] {
    let mut ranges = [(0u16, 0u16); 8];
    let mut at = inner.x + PICKER_MARKER;
    for (index, column) in Column::ALL.iter().enumerate() {
        let end = if index + 1 == Column::ALL.len() {
            inner.right()
        } else {
            at + column.width() as u16 + records::COLUMN_GAP.len() as u16
        };
        ranges[index] = (at, end);
        at = end;
    }
    ranges
}

fn heading_line(sort: Option<(Column, Order)>) -> Line<'static> {
    let cells = records::heading_cells();
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(PICKER_MARKER as usize))];
    for (index, column) in Column::ALL.iter().enumerate() {
        let arrow = match sort {
            Some((sorted, order)) if sorted == *column => match order {
                Order::Ascending => " ↑",
                Order::Descending => " ↓",
            },
            _ => "",
        };
        let text = match column {
            // The last column has no width of its own, so the arrow follows it.
            Column::Checkout => format!("{}{arrow}", cells[index]),
            _ => {
                let marked = format!("{}{arrow}", column.heading());
                format!("{marked:<w$}", w = column.width())
            }
        };
        spans.push(Span::styled(
            text,
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
        if index + 1 < Column::ALL.len() {
            spans.push(Span::raw(records::COLUMN_GAP));
        }
    }
    Line::from(spans)
}

/// Rows the state card takes: its two borders and one line of content.
const NOW_ROWS: u16 = 3;

/// The state card, laid out like the rebuild's own cards: the lock the checkout holds now and the
/// run that built it, so the list says where the reader is standing before it asks them to choose.
fn state_card(frame: &mut Frame<'_>, area: Rect, now: Option<Now<'_>>) {
    let block = panel_block(panel_label("Now"), MUTED, None);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let mut spans = match now {
        Some(now) => vec![
            Span::styled(
                records::head(now.digest, records::LOCK_COLUMN),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ·  {}", now.built), Style::default().fg(MUTED)),
        ],
        None => vec![Span::styled(
            "the checkout has no flake.lock to read",
            Style::default().fg(MUTED),
        )],
    };
    if now.is_none() {
        spans.truncate(1);
    }
    // One cell of air, so the line does not touch the border.
    let text = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), text);
}

/// One row: the table's cells with per-column colours, outcome keeping the run state's colour.
/// The marked row is shaded rather than recoloured; a bullet marks the run that built what the
/// checkout holds now, so the list says where the reader is standing.
fn picker_row(meta: &Meta, selected: bool, current: bool) -> Line<'static> {
    let cells = records::padded_columns(meta);
    let outcome = match meta.exit_code {
        None => YELLOW,
        Some(0) => GREEN,
        Some(_) => RED,
    };
    let color = [
        MUTED,
        Color::Reset,
        MUTED,
        MUTED,
        MUTED,
        outcome,
        MUTED,
        MUTED,
    ];
    let shade = selected.then(|| Style::default().bg(SHADE));
    let gutter = if selected {
        " ❯ "
    } else if current {
        " • "
    } else {
        "   "
    };
    let mut spans = vec![Span::styled(
        gutter.to_owned(),
        match shade {
            Some(shade) => shade.fg(CYAN).add_modifier(Modifier::BOLD),
            None => Style::default().fg(CYAN),
        },
    )];
    for (index, cell) in cells.into_iter().enumerate() {
        let text = match Column::ALL[index] {
            Column::Checkout => cell,
            _ => format!("{cell}{}", records::COLUMN_GAP),
        };
        spans.push(Span::styled(text, Style::default().fg(color[index])));
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

/// The two answers the question offers. Each carries its key, so the box needs no key legend.
const NO_LABEL: &str = "[N  Keep the lock]";
const YES_LABEL: &str = "[Y  Revert and rebuild]";

/// The rollback question over the list: the move it makes and its two keys. It is the app's
/// `Dialog`, the same box `nix-rebuild locks` draws for the same move.
pub fn render_confirm(
    frame: &mut Frame<'_>,
    meta: &Meta,
    kept: Option<&str>,
    hover: Option<Hover>,
) -> Option<dialog::AnswerAreas> {
    let move_line = kept.filter(|digest| !digest.is_empty()).map(|kept| {
        let arriving = records::head(kept, records::LOCK_COLUMN);
        if meta.lock.is_empty() {
            format!("→ {arriving}")
        } else {
            format!(
                "{} → {arriving}",
                records::head(&meta.lock, records::LOCK_COLUMN)
            )
        }
    });
    let mut dialog = Dialog::confirm(
        "Rollback",
        format!("Revert the lock the run of {} replaced?", meta.when()),
    );
    if let Some(move_line) = move_line {
        dialog = dialog.detail(move_line);
    }
    dialog
        .ask(Answers::new(NO_LABEL, YES_LABEL))
        .render(frame, hover, &mut ClickMap::default())
        .answers
}

/// The first row the selector shows, so the chosen one stays in view.
fn picker_offset(selected: usize, total: usize, rows: usize) -> usize {
    if rows == 0 || total <= rows {
        return 0;
    }
    selected.saturating_sub(rows / 2).min(total - rows)
}

/// Draw the selector to a string the way the tests read it; returns the screen and the geometry.
#[cfg(test)]
pub fn picker_to_string(
    runs: &[&Meta],
    selected: usize,
    query: &str,
    filtering: bool,
    sort: Option<(Column, Order)>,
    width: u16,
    height: u16,
) -> (String, PickerGeometry) {
    picker_screen(
        runs,
        selected,
        query,
        filtering,
        sort,
        runs.get(selected).is_some_and(|meta| meta.kept.is_some()),
        None,
        None,
        None,
        None,
        width,
        height,
    )
}

/// The selector with the digest the checkout holds named, so a test can read the mark and the line.
#[cfg(test)]
pub fn picker_to_string_now(
    runs: &[&Meta],
    selected: usize,
    now: &str,
    width: u16,
    height: u16,
) -> String {
    picker_screen(
        runs,
        selected,
        "",
        false,
        None,
        runs.get(selected).is_some_and(|meta| meta.kept.is_some()),
        None,
        None,
        Some(Now {
            digest: now,
            built: "",
        }),
        None,
        width,
        height,
    )
    .0
}

/// The question's frame and where its answers landed, so a test can click them.
#[cfg(test)]
pub fn picker_to_confirm_geometry(
    runs: &[&Meta],
    selected: usize,
    kept: Option<&str>,
    width: u16,
    height: u16,
) -> (String, PickerGeometry) {
    picker_screen(
        runs,
        selected,
        "",
        false,
        None,
        runs.get(selected).is_some_and(|meta| meta.kept.is_some()),
        runs.get(selected).copied(),
        kept,
        None,
        None,
        width,
        height,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn picker_screen(
    runs: &[&Meta],
    selected: usize,
    query: &str,
    filtering: bool,
    sort: Option<(Column, Order)>,
    rollback: bool,
    confirm: Option<&Meta>,
    kept: Option<&str>,
    now: Option<Now<'_>>,
    hover: Option<Hover>,
    width: u16,
    height: u16,
) -> (String, PickerGeometry) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut geometry = None;
    terminal
        .draw(|frame| {
            let view = View {
                query,
                filtering,
                sort,
                rollback,
                confirming: confirm,
                kept,
                now,
                hover,
            };
            geometry = Some(render_picker(frame, runs, selected, &view));
        })
        .expect("render");
    (
        crate::common::ui::buffer_to_string(terminal.backend().buffer()),
        geometry.expect("the frame was drawn"),
    )
}
