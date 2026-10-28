//! What the Logs panel draws: record rows, the plain-text dump for the copy control and `--logs`, and the filter dialog.
//! Each piece receives exactly the state it reads, never the whole model.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};

use crate::common::actions::Command;
use crate::common::hit_targets::{Click, ClickMap, Hover, scrollbar_area, scrollbar_geometry};
use crate::common::layout::{
    centered_rect, content_rows, padded_rows, panel_scrollbar, reserve_scrollbar_column,
    scrollbar_state,
};
use crate::common::nix_events::{LogKind, LogLevel};
use crate::common::text::clip_line;
use crate::common::theme::{BLUE, CYAN, GREEN, MUTED, PURPLE, RED, YELLOW};
use crate::common::ui::{
    ControlSpec, dialog_block, dialog_body, dialog_height, dialog_hint_row, dim_backdrop,
    panel_block, panel_controls, panel_label, render_controls, toggle_line,
};

use super::state::{FILTER_ROWS, FILTERABLE_KINDS, SAVE_ROW, SAVE_SELECTION, State, kind_name};

/// The failure banner's copy button, owned here so the Run card draws it without naming a slice.
/// `copied` is the same confirmation the panel's glyph reads, so both surfaces acknowledge once.
pub fn failure_copy_button(copied: bool) -> (&'static str, crate::common::actions::Command) {
    let label = if copied {
        "[\u{f00c}  Copied]"
    } else {
        "[\u{f0c5}  Copy logs]"
    };
    (label, super::Command::CopyLogs.into())
}

/// The Logs panel's footer control: whether the card is on screen.
pub fn footer_control(show_logs: bool) -> ControlSpec {
    ControlSpec {
        command: super::Command::ToggleLogs.into(),
        prefix: "",
        key: "L",
        icon: Some('\u{f15c}'),
        label: "ogs",
        active: show_logs,
        enabled: true,
    }
}

/// The Logs panel's title actions; raw mode lives in the filter dialog, not here.
pub fn logs_control_specs(state: &State) -> Vec<ControlSpec> {
    vec![
        ControlSpec {
            command: super::Command::OpenLogFilter.into(),
            prefix: "",
            key: "F",
            icon: Some('\u{f0b0}'),
            label: "ilter",
            // Marked active only while a filter actually narrows the view.
            active: !state.filter.is_inactive(),
            enabled: true,
        },
        ControlSpec {
            command: super::Command::CopyLogs.into(),
            prefix: "",
            key: "",
            // The clipboard control is its own confirmation: the glyph changes.
            icon: Some(if state.showing_copied() {
                '\u{f00c}'
            } else {
                '\u{f0c5}'
            }),
            label: "",
            active: true,
            enabled: true,
        },
    ]
}

pub fn render_logs(
    frame: &mut Frame<'_>,
    state: &mut State,
    raw_logs: bool,
    area: Rect,
    hovered: Option<Click>,
) {
    let block = panel_block(
        panel_label("Logs"),
        MUTED,
        Some(panel_controls(render_controls(logs_control_specs(state), hovered)).right_aligned()),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Same viewport size the scroll clamp was given, so render and scroll limit cannot disagree.
    let viewport = state.viewport.max(1);
    let total = state.log_content_len();
    let scrollbar = panel_scrollbar(state.scroll, total, viewport);
    state.scrollbar = scrollbar_geometry(inner, state.scroll, total, viewport);
    let text_area = reserve_scrollbar_column(inner, scrollbar.is_some());
    // One row of air above and below the records, and two columns on each side from the
    // reservation above: the padding every card but the Run card keeps.
    let body = padded_rows(text_area);
    let logs = state.log_window_entries();
    let lines = logs.into_iter().map(|entry| {
        let (icon, label, color) = kind_face(entry.event.kind());
        let message = if raw_logs {
            entry.event.raw()
        } else {
            entry.event.summary()
        };
        let message = trim_severity_prefix(message, entry.event.level);
        clip_line(
            Line::from(vec![
                Span::styled(
                    format!("{icon} {label:<8} "),
                    Style::default().fg(color).bold(),
                ),
                Span::styled(message.to_owned(), Style::default().fg(color)),
            ]),
            body.width,
        )
    });
    // No wrapping: one record is one terminal row, so records, scrollbar, and clamp count the same thing.
    frame.render_widget(Paragraph::new(Text::from_iter(lines)), body);

    if let Some(scrollbar) = scrollbar {
        frame.render_stateful_widget(
            scrollbar,
            scrollbar_area(inner),
            &mut scrollbar_state(state.scroll, total, viewport),
        );
    }
}

fn trim_severity_prefix(message: &str, level: LogLevel) -> &str {
    match level {
        LogLevel::Error => message.strip_prefix("error: ").unwrap_or(message),
        LogLevel::Warning => message.strip_prefix("warning: ").unwrap_or(message),
        LogLevel::Info => message,
    }
}

/// The glyph, word, and colour for a log kind; shared by the pane and the plain-text dump so a kind is never named two ways.
fn kind_face(kind: LogKind) -> (&'static str, &'static str, Color) {
    match kind {
        LogKind::Error => ("\u{f00d}", "error", RED),
        LogKind::Warning => ("\u{f071}", "warning", YELLOW),
        LogKind::Evaluation => ("\u{f002}", "eval", CYAN),
        LogKind::Download => ("\u{f019}", "fetch", BLUE),
        LogKind::Build => ("\u{f0ad}", "build", YELLOW),
        LogKind::Activation => ("\u{f013}", "activate", GREEN),
        LogKind::Cleanup => ("\u{f0e2}", "clean", PURPLE),
        LogKind::Info => ("•", "info", MUTED),
    }
}

/// The Logs pane as plain text: one line per record, its kind word in a column, and the line Nix wrote, whole.
/// Nothing is shortened or width-clipped: this is the copy read in a pipe and grepped for a store path.
pub fn logs_text(state: &State) -> String {
    let mut text = String::new();
    for entry in state.visible_logs() {
        let (_, label, _) = kind_face(entry.event.kind());
        let message = trim_severity_prefix(entry.event.raw(), entry.event.level);
        text.push_str(&format!("{label:<8} {message}\n"));
    }
    text
}

pub fn render_log_filter(
    frame: &mut Frame<'_>,
    state: &mut State,
    raw_logs: bool,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    // Fourteen content rows plus the five of a hint dialog: two borders, the air above the body,
    // the body row left blank above the hint, and the hint row that replaces the bottom air.
    let popup = centered_rect(64, dialog_height(FILTER_ROWS, true), area);
    dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    let title = if state.filter_editing {
        " Filter logs  editing "
    } else {
        " Filter logs "
    };
    let block = dialog_block(clicks, hover, popup, title, CYAN, Command::CloseOverlay);
    let inner = dialog_body(popup);
    frame.render_widget(block, popup);

    let total = state.entries.len();
    let visible = state.visible_logs().count();
    state.filter_rows.push((inner.x, inner.y, inner.width, 1));
    state
        .filter_rows
        .push((inner.x, inner.y.saturating_add(3), inner.width, 1));
    for index in 0..FILTERABLE_KINDS.len() {
        state.filter_rows.push((
            inner.x,
            inner.y.saturating_add(4 + index as u16),
            inner.width,
            1,
        ));
    }
    let query_style = if state.filter_editing || state.filter_row == 0 {
        Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                if state.filter_row == 0 {
                    " \u{276f} query   "
                } else {
                    "   query   "
                },
                query_style,
            ),
            Span::styled(state.filter.query.clone(), query_style),
            Span::styled("▏", query_style),
        ]),
        Line::from(Span::styled(
            format!("           {visible} of {total} records match"),
            Style::default().fg(MUTED),
        )),
        Line::default(),
        toggle_line("Raw log lines", raw_logs, state.filter_row == 1),
    ];
    for (index, (label, kind)) in nix_rebuild_kinds().into_iter().enumerate() {
        lines.push(toggle_line(
            label,
            state.filter.kinds.contains(kind),
            state.filter_row == index + 2,
        ));
    }
    // A blank line, then the button that keeps what this dialog shows for the next start.
    lines.push(Line::default());
    let save_selected = state.filter_row == SAVE_SELECTION;
    lines.push(Line::from(Span::styled(
        format!(
            "   {}",
            if state.saved {
                "[\u{f00c}  Saved]"
            } else {
                "[Save as default]"
            }
        ),
        if save_selected {
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
        } else if state.saved {
            Style::default().fg(GREEN)
        } else {
            Style::default().fg(CYAN)
        },
    )));
    state
        .filter_rows
        .push((inner.x, inner.y.saturating_add(SAVE_ROW), inner.width, 1));
    frame.render_widget(Paragraph::new(Text::from_iter(lines)), inner);

    let hint = if state.filter_editing {
        " type to filter   Enter/Esc done "
    } else {
        " ↑/↓ select   Space toggle   Enter edit   C clear   S save "
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(MUTED))))
            .alignment(Alignment::Center),
        dialog_hint_row(popup),
    );
}

fn nix_rebuild_kinds() -> Vec<(&'static str, LogKind)> {
    FILTERABLE_KINDS
        .into_iter()
        .map(|kind| (kind_name(kind), kind))
        .collect()
}

/// Rows a card can show: height minus two border rows and one blank padding row above and below.
pub fn log_rows(panel: Rect) -> u16 {
    content_rows(panel)
}
