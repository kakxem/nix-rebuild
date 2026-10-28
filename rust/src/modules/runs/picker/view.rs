//! The selector's frame: the run table, its chips, and where they land for hit tests.

use ratatui::Frame;
#[cfg(test)]
use ratatui::Terminal;
#[cfg(test)]
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::common::records::{self, Column, Meta, Order};
use crate::common::theme::{CYAN, GREEN, MUTED, RED, SHADE, YELLOW};

/// A clickable key hint in the title; a click presses the key it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerChip {
    Choose,
    Open,
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
    pub columns: [(u16, u16); 6],
    pub chips: Vec<(PickerChip, u16, u16)>,
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
        if row != self.title_row {
            return None;
        }
        self.chips
            .iter()
            .find(|(_, start, end)| column >= *start && column < *end)
            .map(|(chip, _, _)| *chip)
    }
}

/// Draw the selector; the returned geometry is what the pointer is matched against.
pub fn render_picker(
    frame: &mut Frame<'_>,
    runs: &[&Meta],
    selected: usize,
    query: &str,
    filtering: bool,
    sort: Option<(Column, Order)>,
) -> PickerGeometry {
    let title = if filtering || !query.is_empty() {
        format!(" Recorded runs  /{query} ")
    } else {
        " Recorded runs ".to_owned()
    };
    let chips: &[(PickerChip, &str)] = if filtering {
        &PICKER_SEARCH_CHIPS
    } else {
        &PICKER_CHIPS
    };
    let legend = format!("{} ", legend_of(chips));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(CYAN))
        .title(Span::styled(title, Style::default().fg(CYAN).bold()))
        .title_top(
            Line::from(Span::styled(legend.clone(), Style::default().fg(MUTED))).right_aligned(),
        );
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());

    // The runs lose 3 rows: a blank, the headings, and the blank kept at the bottom.
    let rows = inner.height.saturating_sub(3) as usize;
    let offset = picker_offset(selected, runs.len(), rows);
    let mut lines = vec![Line::default(), heading_line(sort)];
    for (index, meta) in runs.iter().enumerate().skip(offset).take(rows) {
        lines.push(picker_row(meta, index == selected));
    }
    if runs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no run matches the search   Esc clears it",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);

    PickerGeometry {
        title_row: frame.area().y,
        header_row: inner.y + 1,
        first_row: inner.y + 2,
        rows,
        offset,
        total: runs.len(),
        columns: column_ranges(inner),
        chips: chip_ranges(frame.area(), &legend, chips),
    }
}

fn legend_of(chips: &[(PickerChip, &str)]) -> String {
    chips
        .iter()
        .map(|(_, label)| *label)
        .collect::<Vec<_>>()
        .join(" ")
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
fn column_ranges(inner: Rect) -> [(u16, u16); 6] {
    let mut ranges = [(0u16, 0u16); 6];
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

/// One row: the table's cells with per-column colours, outcome keeping the run state's colour.
/// The marked row is shaded rather than recoloured.
fn picker_row(meta: &Meta, selected: bool) -> Line<'static> {
    let cells = records::padded_columns(meta);
    let outcome = match meta.exit_code {
        None => YELLOW,
        Some(0) => GREEN,
        Some(_) => RED,
    };
    let color = [MUTED, Color::Reset, MUTED, outcome, MUTED, MUTED];
    let shade = selected.then(|| Style::default().bg(SHADE));
    let mut spans = vec![Span::styled(
        if selected { " ❯ " } else { "   " }.to_owned(),
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
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut geometry = None;
    terminal
        .draw(|frame| geometry = Some(render_picker(frame, runs, selected, query, filtering, sort)))
        .expect("render");
    (
        crate::common::ui::buffer_to_string(terminal.backend().buffer()),
        geometry.expect("the frame was drawn"),
    )
}
