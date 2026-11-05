//! What the Settings dialog draws: the sections its slices contributed, one row per setting.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};

use super::state::{Line as RowLine, State};
use crate::common::actions::Command;
use crate::common::config::Reading;
use crate::common::dialog;
use crate::common::hit_targets::{ClickMap, Hover, scrollbar_area, scrollbar_geometry};
use crate::common::layout::{centered_rect, panel_scrollbar, scrollbar_state};
use crate::common::text::human_bytes;
use crate::common::theme::{CYAN, GREEN, MUTED, RED};

const LABEL_WIDTH: usize = 27;
/// Where the value column starts, counted from the list's left edge: cursor and padding first.
const VALUE_COLUMN: u16 = 4 + LABEL_WIDTH as u16;
/// The number's column: right-aligned, so `5` and `500` end in the same cell.
const NUMBER_WIDTH: usize = 3;
/// The controls sit in fixed columns, so every number row reads the same and a click on one
/// is a click on the same cell of every row.
const MINUS_COLUMN: u16 = VALUE_COLUMN + NUMBER_WIDTH as u16 + 2;
const PLUS_COLUMN: u16 = MINUS_COLUMN + 4;
/// A button row's chip, drawn where a value would be.
const ACTION_CHIP: &str = "[ Run ]";
/// The box: the label column, the value and its two controls, and the bar's own column.
const DIALOG_WIDTH: u16 = 56;
/// Two border rows, the air inside them, and enough rows to read a section with its title.
const MIN_HEIGHT: u16 = 9;

pub fn render_settings(
    frame: &mut Frame<'_>,
    state: &mut State,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    // One row per line, and every line the slices contributed: the list scrolls when they
    // outgrow the frame, which they do on a short terminal.
    let rows = state.lines().len() as u16;
    let tallest = area.height.saturating_sub(2).max(MIN_HEIGHT);
    let popup = centered_rect(
        DIALOG_WIDTH,
        dialog::height(rows, false).clamp(MIN_HEIGHT, tallest),
        area,
    );
    dialog::dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    let block = dialog::block(
        clicks,
        hover,
        popup,
        " Settings ",
        CYAN,
        Command::CloseOverlay,
    );
    let list = dialog::body(popup);
    frame.render_widget(block, popup);

    state.set_offset(list.height as usize);
    let lines = state.lines();
    let viewport = list.height.max(1) as usize;
    // The dialog is read from the top, so the bar is given the inverted offset it expects:
    // first line at the top of the track, last line at the bottom.
    let max = lines.len().saturating_sub(viewport);
    let bar_offset = max.saturating_sub(state.offset);
    let track = list;
    let scrollbar = panel_scrollbar(bar_offset, lines.len(), viewport);
    state.scrollbar = scrollbar_geometry(track, bar_offset, lines.len(), viewport);
    let list = Rect {
        width: list
            .width
            .saturating_sub(if scrollbar.is_some() { 2 } else { 0 }),
        ..list
    };
    state.area = Some((list.x, list.y, list.width, list.height));

    let entries = state.entries.clone();
    let config = state.config.clone();
    let mut buttons = Vec::new();
    let mut drawn = Vec::new();
    for (row_index, line) in lines
        .iter()
        .skip(state.offset)
        .take(list.height as usize)
        .enumerate()
    {
        let y = list.y + row_index as u16;
        match line {
            RowLine::Gap => drawn.push(Line::default()),
            RowLine::Section(title) => drawn.push(Line::from(Span::styled(
                format!("  {title}"),
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ))),
            RowLine::Entry(index) => {
                let entry = entries[*index];
                let selected = *index == state.row;
                let (cursor, cursor_style) = if selected {
                    ('\u{276f}', Style::default().fg(CYAN).bold())
                } else {
                    (' ', Style::default().fg(MUTED))
                };
                let label_style = if selected {
                    Style::default().fg(GREEN)
                } else {
                    Style::default().fg(MUTED)
                };
                let value_style = Style::default().fg(if selected { CYAN } else { MUTED });
                let mut spans = vec![
                    Span::styled(format!("  {cursor} "), cursor_style),
                    Span::styled(format!("{:<LABEL_WIDTH$}", entry.label), label_style),
                ];
                if entry.is_number() {
                    spans.push(Span::styled(
                        format!("{:>NUMBER_WIDTH$}", entry.shown(&config)),
                        value_style,
                    ));
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled("[-]", Style::default().fg(CYAN)));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled("[+]", Style::default().fg(CYAN)));
                    // The controls are hit areas of their own, so a click steps instead of selecting.
                    buttons.push(((list.x + MINUS_COLUMN, y, 3, 1), -1));
                    buttons.push(((list.x + PLUS_COLUMN, y, 3, 1), 1));
                } else if entry.action_command().is_some() {
                    // A button row says what it is by its shape, and the row itself is the target.
                    spans.push(Span::styled(
                        ACTION_CHIP,
                        if selected {
                            Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(MUTED)
                        },
                    ));
                } else if let Some(reading) = entry.reading() {
                    // A figure the dialog holds: nothing to press, so it never reads as editable.
                    let shown = match reading {
                        Reading::RecordedRuns => format!("{}", state.recorded_runs),
                        Reading::RecordedBytes => human_bytes(state.recorded_bytes),
                    };
                    spans.push(Span::styled(shown, Style::default().fg(MUTED)));
                } else {
                    // A toggle reads green when it is on and red when it is off.
                    let styled = match entry.is_on(&config) {
                        Some(true) => Span::styled("on".to_owned(), Style::default().fg(GREEN)),
                        Some(false) => Span::styled("off".to_owned(), Style::default().fg(RED)),
                        None => Span::styled(entry.shown(&config), value_style),
                    };
                    spans.push(styled);
                }
                drawn.push(Line::from(spans));
            }
        }
    }
    state.buttons = buttons;
    frame.render_widget(Paragraph::new(Text::from_iter(drawn)), list);

    if let Some(scrollbar) = scrollbar {
        frame.render_stateful_widget(
            scrollbar,
            scrollbar_area(track),
            &mut scrollbar_state(bar_offset, lines.len(), viewport),
        );
    }
}
