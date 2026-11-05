//! What the Activity panel draws: the work rows and the filter dialog.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Clear, Paragraph, Row, Table};

use crate::common::actions::Command;
use crate::common::dialog;
use crate::common::hit_targets::{Click, ClickMap, Hover, scrollbar_area, scrollbar_geometry};
use crate::common::layout::{
    centered_rect, content_rows, padded_rows, panel_scrollbar, reserve_scrollbar_column,
    scrollbar_state,
};
use crate::common::phase::Phase;
use crate::common::text::{animation_tick, format_clock, human_bytes};
use crate::common::theme::{BLUE, CYAN, GREEN, MUTED, PURPLE, RED, SPINNER_FRAMES, YELLOW};
use crate::common::ui::{
    ControlSpec, panel_block, panel_controls, panel_label, render_controls, toggle_line,
};

use super::state::{
    ACTIVITY_FILTER_ROWS, Activity, ActivityKind, ActivityState, SAVE_ROW, SAVE_SELECTION, State,
};

/// The glyph column: one cell, so the package name starts one space after its icon.
const ICON_WIDTH: u16 = 1;
/// The state column: a spinner, a space, and the longest word Nix has ("downloading").
const STATE_WIDTH: u16 = 15;
/// The clock column: `mm:ss`, and `h:mm:ss` once a run is long.
const CLOCK_WIDTH: u16 = 8;
/// The transfer column: two byte counts, `84.0 MiB/132.0 MiB`, as wide as they get.
const TRANSFER_WIDTH: u16 = 19;

/// The Activity panel's footer control: whether the card is on screen.
pub fn footer_control(show_activity: bool) -> ControlSpec {
    ControlSpec {
        command: super::Command::ToggleActivity.into(),
        prefix: "",
        key: "A",
        icon: Some('\u{f0ae}'),
        label: "ctivity",
        active: show_activity,
        enabled: true,
    }
}

/// The panel's toggles: which kinds are listed, a way between the two views, and the way out
/// of a running rebuild. Which chips exist follows the view being drawn, not the phase, so the
/// history keeps its filter while the cleanup runs.
pub fn activity_control_specs(
    cleaning: bool,
    running: bool,
    show_cleanup_progress: bool,
    show_downloads: bool,
    show_builds: bool,
    show_activity_history: bool,
) -> Vec<ControlSpec> {
    let mut specs = Vec::new();
    if !(cleaning && show_cleanup_progress) {
        specs.push(ControlSpec {
            command: super::Command::OpenActivityFilter.into(),
            prefix: "Fil",
            key: "t",
            icon: Some('\u{f0b0}'),
            label: "er",
            active: !show_downloads || !show_builds || show_activity_history,
            enabled: true,
        });
    }
    if cleaning {
        specs.push(ControlSpec {
            command: super::Command::ToggleActivityView.into(),
            prefix: "",
            key: "v",
            icon: Some('\u{f021}'),
            label: if show_cleanup_progress {
                " Show work"
            } else {
                " Show cleanup"
            },
            active: !show_cleanup_progress,
            enabled: true,
        });
    }
    // Same command the key sends: the question comes up first, so a click cannot cancel blind.
    if running {
        specs.push(ControlSpec {
            command: crate::common::actions::Command::request_cancel(),
            prefix: "",
            key: "Esc",
            icon: None,
            label: "",
            active: false,
            enabled: true,
        });
    }
    specs
}
/// Draw the Activity card's frame and body; cleanup steps go to the caller's `cleanup_view`.
#[allow(clippy::too_many_arguments)]
pub fn render_activity(
    frame: &mut Frame<'_>,
    state: &mut State,
    area: Rect,
    hovered: Option<Click>,
    phase: Phase,
    status: &str,
    rebuild_failed: bool,
    show_cleanup_progress: bool,
    show_downloads: bool,
    show_builds: bool,
    show_activity_history: bool,
    cleanup_view: impl FnOnce(&mut Frame<'_>, Rect),
) {
    let border_color = if phase == Phase::Cleanup {
        PURPLE
    } else {
        MUTED
    };
    let controls = render_controls(
        activity_control_specs(
            phase == Phase::Cleanup,
            phase.is_running(),
            show_cleanup_progress,
            show_downloads,
            show_builds,
            show_activity_history,
        ),
        hovered,
    );
    let trailing = (!controls.is_empty()).then(|| panel_controls(controls).right_aligned());
    let title = match state.build_progress {
        Some((done, expected)) => format!("Activity \u{b7} {done}/{expected}"),
        None => "Activity".to_owned(),
    };
    let block = panel_block(panel_label(&title), border_color, trailing);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // A failed rebuild hands the body to its banner instead of the history.
    if rebuild_failed {
        return;
    }
    if phase == Phase::Cleanup && show_cleanup_progress {
        cleanup_view(frame, inner);
        return;
    }
    if inner.height == 0 {
        return;
    }

    // The same row count the scroll clamp uses, so the two cannot disagree.
    let available = state.viewport.max(1);
    let visible_len = state
        .visible_activities(show_builds, show_downloads)
        .count();
    state.scrollbar = scrollbar_geometry(inner, state.scroll, visible_len, available);
    let visible: Vec<_> = state
        .visible_activities(show_builds, show_downloads)
        .collect();
    if visible.is_empty() {
        let message: String = match phase {
            Phase::Complete => "no build or download activity".into(),
            Phase::Failed => String::new(),
            Phase::Cancelled if status.starts_with("cleanup cancelled") => {
                "cleanup cancelled — rebuild remains active".into()
            }
            Phase::Cancelled => "rebuild cancelled".into(),
            Phase::Evaluating => "evaluating the flake…".into(),
            Phase::Activating => "activating the new generation…".into(),
            _ => "waiting for Nix activity…".into(),
        };
        if !message.is_empty() {
            let color = match phase {
                Phase::Complete => GREEN,
                Phase::Cancelled => RED,
                _ => MUTED,
            };
            // A running step keeps the spinner so the wait is visible before Nix reports.
            let message = if phase.is_running() {
                format!(
                    "{} {message}",
                    SPINNER_FRAMES[animation_tick() % SPINNER_FRAMES.len()]
                )
            } else {
                message
            };
            frame.render_widget(
                Paragraph::new(Span::styled(message, Style::default().fg(color)))
                    .alignment(Alignment::Center),
                centered_line(inner),
            );
        }
        return;
    }

    // `visible` is oldest-first: take the window from the newest end, then restore display order.
    let rows = visible
        .iter()
        .rev()
        .skip(state.scroll)
        .take(available)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|activity| activity_row(activity));
    // Reserve the scrollbar column so it never sits on the last table column.
    let scrollbar = panel_scrollbar(state.scroll, visible_len, available);
    let table_area = padded_rows(reserve_scrollbar_column(inner, scrollbar.is_some()));
    // The width is solved from the card, not from percentages of it: the glyph is one cell, the
    // name takes the slack between it and the numbers, and the transfer column ends at the card's
    // edge, so a resize never leaves a strip of empty cells after the last column.
    let widths = [
        Constraint::Length(ICON_WIDTH),
        Constraint::Fill(1),
        Constraint::Length(STATE_WIDTH),
        Constraint::Length(CLOCK_WIDTH),
        Constraint::Length(TRANSFER_WIDTH),
    ];
    // The bytes are a number, so the column ends at the card's edge and its figures line up.
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("package"),
        Cell::from("state"),
        Cell::from("time"),
        Cell::from(Text::from("transfer").alignment(Alignment::Right)),
    ])
    .style(Style::default().fg(MUTED));
    let table = Table::new(rows, widths).column_spacing(1).header(header);
    frame.render_widget(table, table_area);

    if let Some(scrollbar) = scrollbar {
        frame.render_stateful_widget(
            scrollbar,
            scrollbar_area(inner),
            &mut scrollbar_state(state.scroll, visible_len, available),
        );
    }
}
fn activity_row(activity: &Activity) -> Row<'static> {
    let (icon, color) = match activity.state {
        ActivityState::Done => ("", GREEN),
        ActivityState::Failed => ("", RED),
        ActivityState::NotBuilt => ("", MUTED),
        ActivityState::Running if activity.kind == ActivityKind::Source => ("\u{f0c5}", CYAN),
        ActivityState::Running if activity.kind == ActivityKind::Probe => ("\u{f002}", CYAN),
        ActivityState::Queued => ("", MUTED),
        ActivityState::Running if activity.kind == ActivityKind::Download => ("", BLUE),
        ActivityState::Running => ("", YELLOW),
    };
    let state = match activity.state {
        ActivityState::Done => "done".into(),
        ActivityState::Failed => "failed".into(),
        ActivityState::NotBuilt => "not built".into(),
        ActivityState::Queued => "queued".into(),
        ActivityState::Running => format!(
            "{} {}",
            SPINNER_FRAMES[animation_tick() % SPINNER_FRAMES.len()],
            running_state(activity)
        ),
    };
    Row::new(vec![
        Cell::from(icon).style(Style::default().fg(color)),
        Cell::from(activity.name.clone()),
        Cell::from(state).style(Style::default().fg(color)),
        Cell::from(format_clock(activity.elapsed_at(Instant::now())))
            .style(Style::default().fg(MUTED)),
        Cell::from(Text::from(transfer_cell(activity)).alignment(Alignment::Right)).style(
            Style::default().fg(if activity.kind == ActivityKind::Download {
                BLUE
            } else {
                MUTED
            }),
        ),
    ])
}
fn running_state(activity: &Activity) -> String {
    if !activity.phase.is_empty() {
        return activity.phase.clone();
    }
    match activity.kind {
        ActivityKind::Download => "downloading".into(),
        ActivityKind::Build => "building".into(),
        ActivityKind::Source => "copying".into(),
        ActivityKind::Probe => "querying".into(),
    }
}
/// Bytes a transfer moved, or the paths an aggregate counts.
/// Builds and unmeasured rows stay empty rather than show an invented percentage.
fn transfer_cell(activity: &Activity) -> String {
    if matches!(activity.kind, ActivityKind::Source | ActivityKind::Probe) {
        return count_cell(activity.units);
    }
    if activity.kind == ActivityKind::Build || (activity.units == 0 && activity.total == 0) {
        return String::new();
    }
    if activity.state == ActivityState::Done {
        return human_bytes(activity.total.max(activity.units));
    }
    if activity.total > 0 {
        return format!(
            "{}/{}",
            human_bytes(activity.units),
            human_bytes(activity.total)
        );
    }
    human_bytes(activity.units)
}
/// A path count with its unit, so the column never reads as bytes.
fn count_cell(count: u64) -> String {
    match count {
        0 => String::new(),
        1 => "1 path".to_owned(),
        count => format!("{count} paths"),
    }
}
#[allow(clippy::too_many_arguments)]
pub fn render_activity_filter(
    frame: &mut Frame<'_>,
    state: &mut State,
    show_downloads: bool,
    show_builds: bool,
    show_activity_history: bool,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    let popup = centered_rect(54, dialog::height(ACTIVITY_FILTER_ROWS, true), area);
    dialog::dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    let block = dialog::block(
        clicks,
        hover,
        popup,
        " Filter ".to_owned(),
        CYAN,
        Command::CloseOverlay,
    );
    let inner = dialog::body(popup);
    frame.render_widget(block, popup);
    state.filter_rows = (0..3)
        .map(|index| (inner.x, inner.y + index, inner.width, 1))
        .collect();
    state
        .filter_rows
        .push((inner.x, inner.y + SAVE_ROW, inner.width, 1));

    // Fetching covers packages, copied sources and cache probes.
    let downloads = state
        .active
        .values()
        .chain(state.finished.iter())
        .filter(|activity| {
            matches!(
                activity.kind,
                ActivityKind::Download | ActivityKind::Source | ActivityKind::Probe
            )
        })
        .count();
    let builds = state
        .active
        .values()
        .chain(state.finished.iter())
        .filter(|activity| activity.kind == ActivityKind::Build)
        .count();
    let lines = vec![
        toggle_line(
            &format!("downloads  ({downloads})"),
            show_downloads,
            state.filter_row == 0,
        ),
        toggle_line(
            &format!("builds     ({builds})"),
            show_builds,
            state.filter_row == 1,
        ),
        toggle_line(
            &format!("completed  ({})", state.finished.len()),
            show_activity_history,
            state.filter_row == 2,
        ),
        Line::default(),
        Line::from(Span::styled(
            if state.saved {
                "[\u{f00c}  Saved]"
            } else {
                "[Save as default]"
            },
            if state.filter_row == SAVE_SELECTION {
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
            } else if state.saved {
                Style::default().fg(GREEN)
            } else {
                Style::default().fg(CYAN)
            },
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " ↑/↓ select   Space/Enter toggle   S save ",
            Style::default().fg(MUTED),
        )))
        .alignment(Alignment::Center),
        dialog::hint_row(popup),
    );
}

/// Rows a card can show: two borders and one table header.
pub fn activity_rows(panel: Rect) -> u16 {
    // The header row is the only thing the card's content rows lose.
    content_rows(panel).saturating_sub(1)
}

fn centered_line(area: Rect) -> Rect {
    Rect::new(area.x, area.y + area.height / 2, area.width, 1)
}
