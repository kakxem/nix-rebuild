//! The view composition root: which module paints which rectangle, and where overlays land.
//! The overlay routing lives here because an overlay borrows another module's card.

mod controls;
mod panels;
pub(crate) mod start_cards;
#[cfg(test)]
mod testing;

#[cfg(test)]
mod tests;

use crate::common::panels::PanelSpec;
pub use controls::completion_line;
pub use panels::{Heights, PanelId, Panels, Resize, Split};
#[cfg(test)]
pub use testing::{render_to_string, render_to_string_with_state};

use controls::{footer_control_specs, record_control_clicks, record_footer_clicks, render_footer};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::model::{App, Overlay};
use crate::common::layout::{centered_block, panel_body};
use crate::common::phase::Phase;
use crate::common::ui::render_controls;
use crate::modules::panels::activity::{
    self, activity_control_specs, render_activity, render_activity_filter,
};
use crate::modules::panels::help;
use crate::modules::panels::logs::{
    self, failure_copy_button, logs_control_specs, render_log_filter, render_logs,
};
use crate::modules::panels::status;
use crate::modules::work::cleanup;

use crate::modules::panels::settings;
use crate::modules::runs::records;

/// Splits the terminal between the cards and the footer; one free row separates them.
fn dashboard_layout(area: Rect) -> (Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    (rows[0], rows[2])
}

pub fn render(frame: &mut Frame<'_>, app: &mut App) -> Panels {
    let frame_area = frame.area();
    let area = if frame_area.height > 1 {
        Rect {
            height: frame_area.height - 1,
            ..frame_area
        }
    } else {
        frame_area
    };
    let (content, footer) = dashboard_layout(area);
    let specs = panel_specs(app);
    // Pinned heights belong to one arrangement in one window: a new size or a card shown or
    // hidden gets the default split back, so a drag never leaves rows unallocated.
    let layout_key = (content.height, specs);
    if app.layout_key != Some(layout_key) {
        app.layout_key = Some(layout_key);
        app.heights = Heights::default();
    }
    let panels = Panels::layout(content, &specs, app.heights);
    app.logs.scrollbar = None;
    app.activity.scrollbar = None;
    app.settings.scrollbar = None;
    // Row counts first: the panels render against them and the scroll clamp uses them.
    if app.config.show_activity {
        app.activity.set_activity_viewport(
            activity::activity_rows(panels.rect(PanelId::Activity)) as usize,
            app.config.show_builds,
            app.config.show_downloads,
        );
    }
    if app.config.show_logs {
        // `Borders::TOP` costs one row; a blank row pads the bottom.
        app.logs
            .set_log_viewport(logs::log_rows(panels.rect(PanelId::Logs)) as usize);
    }
    // Hover comes from the previous frame's click map, so controls draw already highlighted.
    let hovered = app.hovered_click();
    if app.config.show_status {
        status::render_summary(
            frame,
            &app.status,
            app.cleanup.freed.as_ref(),
            app.cleanup.succeeded,
            panels.rect(PanelId::Status),
        );
    }
    if app.config.show_activity {
        let rebuild_failed = app.rebuild_failed();
        let cleanup_view = |frame: &mut Frame<'_>, inner: Rect| {
            cleanup::render_cleanup_activity(frame, &app.cleanup, inner)
        };
        render_activity(
            frame,
            &mut app.activity,
            panels.rect(PanelId::Activity),
            hovered,
            app.status.phase,
            &app.status.message,
            rebuild_failed,
            app.config.show_cleanup_progress,
            app.config.show_downloads,
            app.config.show_builds,
            app.config.show_activity_history,
            cleanup_view,
        );
    }
    if app.config.show_logs {
        render_logs(
            frame,
            &mut app.logs,
            app.config.raw_logs,
            panels.rect(PanelId::Logs),
            hovered,
        );
    }
    // The closing line goes on the footer row with the gap above left empty: no rows
    // from the cards, and nothing under the frame, which would scroll it.
    let closing = app.closing && app.run_id.is_some();
    if closing && let Some(id) = app.run_id.as_deref() {
        records::render_closing(frame, id, footer);
    } else {
        render_footer(frame, app, footer, hovered);
    }

    // Click map rebuilt from the spans the panels drew; positions are absolute screen cells.
    app.clicks.clear();
    if app.config.show_activity {
        record_control_clicks(
            app,
            panels.rect(PanelId::Activity),
            &render_controls(
                activity_control_specs(
                    app.status.phase == Phase::Cleanup,
                    app.status.phase.is_running(),
                    app.config.show_cleanup_progress,
                    app.config.show_downloads,
                    app.config.show_builds,
                    app.config.show_activity_history,
                ),
                hovered,
            ),
        );
    }
    if app.config.show_logs {
        record_control_clicks(
            app,
            panels.rect(PanelId::Logs),
            &render_controls(logs_control_specs(&app.logs), hovered),
        );
    }
    if !closing {
        record_footer_clicks(
            app,
            footer,
            &render_controls(footer_control_specs(app), hovered),
        );
    }

    app.logs.filter_rows.clear();
    app.activity.filter_rows.clear();
    match app.overlay {
        Overlay::ConfirmCancel => status::render_confirm_cancel(
            frame,
            content,
            app.status.phase,
            app.hover,
            &mut app.clicks,
        ),
        // One surface whatever the cards show: the decision changes the machine, so it is
        // never handed to a card the user may have closed.
        Overlay::CleanupPrompt => {
            cleanup::render_cleanup_dialog(frame, content, app.hover, &mut app.clicks)
        }
        Overlay::Help => {
            help::render_help(frame, &mut app.help, app.hover, &mut app.clicks, content)
        }
        Overlay::Details => status::render_details(
            frame,
            &app.status,
            app.run_id.as_deref(),
            app.cleanup.policy.label(),
            &[
                ("logs", app.logs.entries.len()),
                ("activities", app.activity.finished.len()),
            ],
            app.hover,
            &mut app.clicks,
            content,
        ),
        Overlay::LogFilter => render_log_filter(
            frame,
            &mut app.logs,
            app.config.raw_logs,
            app.hover,
            &mut app.clicks,
            content,
        ),
        // The question about the journal is the app's too: the button that asks for it
        // lives on a card the user may have closed.
        Overlay::RemoveRunsPrompt => records::render_remove_dialog(
            frame,
            app.records.count,
            app.hover,
            &mut app.clicks,
            content,
        ),
        Overlay::ActivityFilter => render_activity_filter(
            frame,
            &mut app.activity,
            app.config.show_downloads,
            app.config.show_builds,
            app.config.show_activity_history,
            app.hover,
            &mut app.clicks,
            content,
        ),
        // The file's defaults, edited in a dialog: the frame behind it is the run's.
        Overlay::Settings => settings::render_settings(
            frame,
            &mut app.settings,
            app.hover,
            &mut app.clicks,
            content,
        ),
        // A failed rebuild gives its Activity card to the banner, centered in it.
        // With the card hidden the banner is drawn as a dialog of its own.
        Overlay::None => {
            if app.rebuild_failed() {
                let copy_button = failure_copy_button(app.logs.showing_copied());
                if app.config.show_activity {
                    status::render_failure_prompt(
                        frame,
                        &app.status,
                        Some(copy_button),
                        app.hover,
                        &mut app.clicks,
                        centered_block(
                            panel_body(panels.rect(PanelId::Activity)),
                            status::FAILURE_BANNER_ROWS,
                        ),
                    );
                } else {
                    status::render_failure_dialog(
                        frame,
                        &app.status,
                        Some(copy_button),
                        app.hover,
                        &mut app.clicks,
                        content,
                    );
                }
            }
        }
    }
    panels
}

/// Each card declares its own place in the stack; this is the order `PanelId::ALL` names.
/// Index-aligned with `PanelId::ALL`: each card declares its own row, the order is not restated here.
pub(crate) fn panel_specs(app: &App) -> [PanelSpec; PanelId::ALL.len()] {
    [
        status::panel_spec(app.config.show_status),
        activity::panel_spec(app.config.show_activity),
        logs::panel_spec(app.config.show_logs),
    ]
}
