//! Footer buttons and their hit maps; the shared control silhouette lives in `crate::common::ui`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::model::{App, Overlay};
use crate::common::actions::Command;
use crate::common::hit_targets::Click;
use crate::common::phase::Phase;
use crate::common::theme::GREEN;
use crate::common::ui::{ControlSpec, PANEL_CONTROL_INSET, control_width, render_controls};
use crate::modules::panels::activity;
use crate::modules::panels::help;
use crate::modules::panels::logs;
use crate::modules::panels::settings;
use crate::modules::panels::status;

/// The controls that ride the right edge: Help beside Settings, so the two overlays sit together.
const ANCHORED: usize = 2;

pub(super) fn footer_control_specs(app: &App) -> Vec<ControlSpec> {
    // Card order, top to bottom; Help and Settings ride the right edge.
    vec![
        status::footer_control(app.config.show_status),
        activity::footer_control(app.config.show_activity),
        logs::footer_control(app.config.show_logs),
        help::footer_control(),
        settings::footer_control(),
    ]
}

/// The column each control starts at, in the order given: the cards from the left edge, the
/// anchored group flush right. Painting and the hit map both read this, so they cannot drift.
fn control_cells(area: Rect, controls: &[(ControlSpec, Vec<Span<'static>>)]) -> Vec<Option<u16>> {
    let widths: Vec<u16> = controls
        .iter()
        .map(|(_, chips)| control_width(chips))
        .collect();
    let mut cells = vec![None; controls.len()];
    if area.width == 0 || controls.is_empty() {
        return cells;
    }
    let split = controls.len().saturating_sub(ANCHORED);
    // One cell of air inside the frame's right edge, and one between the anchored controls.
    let mut right = area.right().saturating_sub(1);
    for index in (split..controls.len()).rev() {
        let x = right.saturating_sub(widths[index]);
        if x < area.x {
            break;
        }
        cells[index] = Some(x);
        right = x.saturating_sub(1);
    }
    // The cards stop a cell short of the anchored group rather than running into it.
    let limit = cells
        .iter()
        .flatten()
        .min()
        .copied()
        .unwrap_or(area.right());
    let mut cursor = area.x.saturating_add(1);
    for index in 0..split {
        if cursor.saturating_add(widths[index]) >= limit {
            break;
        }
        cells[index] = Some(cursor);
        cursor = cursor.saturating_add(widths[index]).saturating_add(1);
    }
    cells
}

/// The cells the anchored group takes, gaps included, so nothing runs into it.
fn anchored_width(controls: &[(ControlSpec, Vec<Span<'static>>)]) -> u16 {
    let split = controls.len().saturating_sub(ANCHORED);
    let cells: u16 = controls[split..]
        .iter()
        .map(|(_, chips)| control_width(chips))
        .sum();
    cells.saturating_add(ANCHORED.saturating_sub(1) as u16)
}

/// Rects derived from the spans the renderer builds, so the hit map cannot drift from the screen.
pub(super) fn record_control_clicks(
    app: &mut App,
    panel: Rect,
    controls: &[(ControlSpec, Vec<Span<'static>>)],
) {
    if panel.width == 0 || controls.is_empty() {
        return;
    }
    let row = panel.y;

    // Measure each control exactly as `panel_controls` wraps it.
    let widths: Vec<u16> = controls
        .iter()
        .map(|(_, chips)| control_width(chips))
        .collect();
    let gaps: u16 = controls.len().saturating_sub(1) as u16; // one space between
    let leading: u16 = 1; // the space before the first control
    let total: u16 = widths.iter().sum::<u16>() + gaps + leading + PANEL_CONTROL_INSET;

    // Right-aligned inside the card's right border; the leading space and inset are not part of a button.
    let content_right = panel.right().saturating_sub(1);
    let line_start = content_right.saturating_sub(total);
    let mut cursor = line_start.saturating_add(leading);
    for ((spec, _), width) in controls.iter().zip(widths.iter()) {
        if cursor.saturating_add(*width) > content_right {
            break;
        }
        app.clicks
            .push(((cursor, row, *width, 1), Click::Command(spec.command)));
        cursor = cursor.saturating_add(*width).saturating_add(1); // gap
    }
}

fn completion_footer_area(area: Rect, anchored: u16) -> Rect {
    let width = UnicodeWidthStr::width(COMPLETION_BUTTON) as u16;
    Rect::new(
        area.right()
            .saturating_sub(1)
            .saturating_sub(anchored)
            .saturating_sub(1)
            .saturating_sub(width),
        area.y,
        width,
        1,
    )
}

/// From the same spans used for painting: the cards left-aligned, the overlays anchored right.
pub(super) fn record_footer_clicks(
    app: &mut App,
    area: Rect,
    controls: &[(ControlSpec, Vec<Span<'static>>)],
) {
    if area.width == 0 || area.height == 0 || controls.is_empty() {
        return;
    }
    for ((spec, chips), cell) in controls.iter().zip(control_cells(area, controls)) {
        let Some(x) = cell else {
            continue;
        };
        let width = control_width(chips);
        let click = if spec.command == Command::from(help::Command::OpenHelp) {
            Click::Help
        } else {
            Click::Command(spec.command)
        };
        app.clicks.push(((x, area.y, width, 1), click));
    }
    if app.status.phase == Phase::Complete && app.overlay == Overlay::None {
        let button = completion_footer_area(area, anchored_width(controls));
        app.clicks.push((
            (button.x, button.y, button.width, button.height),
            Click::Command(Command::Quit),
        ));
    }
}

pub(super) fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect, hovered: Option<Click>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let controls = render_controls(footer_control_specs(app), hovered);
    let cells = control_cells(area, &controls);
    let split = controls.len().saturating_sub(ANCHORED);
    // Where the cards' own controls end, so the closing button never lands on top of one.
    let mut left_end = area.x;
    for (index, ((_, chips), cell)) in controls.iter().zip(cells).enumerate() {
        let Some(x) = cell else {
            continue;
        };
        let width = control_width(chips);
        if x.saturating_add(width) <= area.right() {
            frame.render_widget(
                Paragraph::new(Line::from(chips.clone())),
                Rect::new(x, area.y, width, 1),
            );
        }
        if index < split {
            left_end = left_end.max(x.saturating_add(width));
        }
    }
    if app.status.phase == Phase::Complete && app.overlay == Overlay::None {
        let button = completion_footer_area(area, anchored_width(&controls));
        if button.x >= left_end {
            frame.render_widget(
                Paragraph::new(completion_line_with_hover(
                    hovered == Some(Click::Command(Command::Quit)),
                )),
                button,
            );
        }
    }
}

const COMPLETION_BUTTON: &str = "[Enter  Close]";

/// The close key is emphasised so it reads as an action, not part of the sentence.
fn completion_line_with_hover(hovered: bool) -> Line<'static> {
    let button_style = if hovered {
        Style::default()
            .fg(GREEN)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED | Modifier::REVERSED)
    } else {
        Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
    };
    Line::from(Span::styled(COMPLETION_BUTTON, button_style))
}

pub fn completion_line() -> Line<'static> {
    completion_line_with_hover(false)
}
