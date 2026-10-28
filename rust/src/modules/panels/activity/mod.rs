//! The Activity panel: the rows the run's work becomes, its dedup, scroll and filter dialog.

mod state;
mod view;

pub use state::{ACTIVITY_FILTER_ROWS, SAVE_ROW, SAVE_SELECTION};
pub use state::{Activity, ActivityKind, ActivityState, Started, State, settings_entries};
pub(crate) use state::{probe_host, transfer_subject};
pub use view::{
    activity_control_specs, activity_rows, footer_control, render_activity, render_activity_filter,
};

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::common::actions;
use crate::common::config::Settings;
use crate::common::nix_events::store_name;
use crate::common::panels::{Height, PanelSpec};
use crate::common::ui::HelpEntry;

/// The Activity card takes whatever the cards above leave, and never drops under five rows.
pub fn panel_spec(visible: bool) -> PanelSpec {
    PanelSpec::new(Height::Fill { floor: 5 }, visible)
}

/// The panel's rows in the keyboard help overlay, owned here so the overlay never names this slice.
pub fn help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry {
            key: "A",
            label: "Activity",
            command: Some(Command::ToggleActivity.into()),
        },
        HelpEntry {
            key: "T",
            label: "Filter activity",
            command: Some(Command::OpenActivityFilter.into()),
        },
        HelpEntry {
            key: "V",
            label: "Swap the Activity card between work and the cleanup steps",
            command: Some(Command::ToggleActivityView.into()),
        },
    ]
}

/// `querying info about '<path>' on '<cache>'`: a metadata probe, not a transfer, so it owns no row.
const QUERY_PATH_INFO: i64 = 109;

/// Nix's informational messages: text only, no fields to name a path or bytes.
const PLAIN_MESSAGE: i64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    ToggleActivity,
    ToggleActivityView,
    ToggleFetch,
    ToggleBuilds,
    ToggleActivityHistory,
    OpenActivityFilter,
    ScrollActivity(i16),
    SetActivityScroll(usize),
    /// Keep the choosers the dialog shows as what a start begins with.
    SaveFilter,
}

/// What the panel asks of the runtime once its state is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    OpenFilter,
    /// Hand the three choosers to the file's copy; the runtime writes it.
    SaveFilter {
        show_downloads: bool,
        show_builds: bool,
        show_activity_history: bool,
    },
}

/// Apply one Activity command and report its effects.
pub fn reduce(state: &mut State, settings: &mut Settings, command: Command) -> Vec<Effect> {
    // The view toggles belong to this frame only; the file moves from the Settings dialog.
    match command {
        Command::ToggleActivity => {
            settings.show_activity = !settings.show_activity;
            vec![]
        }
        Command::ToggleActivityView => {
            settings.show_cleanup_progress = !settings.show_cleanup_progress;
            vec![]
        }
        Command::ToggleFetch => {
            settings.show_downloads = !settings.show_downloads;
            state.saved = false;
            vec![]
        }
        Command::ToggleBuilds => {
            settings.show_builds = !settings.show_builds;
            state.saved = false;
            vec![]
        }
        Command::ToggleActivityHistory => {
            settings.show_activity_history = !settings.show_activity_history;
            state.saved = false;
            vec![]
        }
        Command::OpenActivityFilter => {
            state.filter_row = 0;
            state.saved = false;
            vec![Effect::OpenFilter]
        }
        Command::SaveFilter => {
            state.saved = true;
            vec![Effect::SaveFilter {
                show_downloads: settings.show_downloads,
                show_builds: settings.show_builds,
                show_activity_history: settings.show_activity_history,
            }]
        }
        Command::ScrollActivity(rows) => {
            state.scroll_activities_by(rows as i64, settings.show_builds, settings.show_downloads);
            vec![]
        }
        Command::SetActivityScroll(offset) => {
            state.set_activity_scroll(offset, settings.show_builds, settings.show_downloads);
            vec![]
        }
    }
}

/// `Output` and `NixJson` arrive through [`apply_json`] instead; other actions are ignored.
pub fn handle_action(_state: &mut State, _action: &actions::Action) -> Vec<Effect> {
    Vec::new()
}

pub fn key_command(
    state: &mut State,
    key: KeyEvent,
    dialog_open: bool,
) -> Option<actions::Command> {
    if dialog_open {
        filter_key(state, key)
    } else {
        frame_key(key)
    }
}

pub fn mouse_command(
    state: &mut State,
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
) -> Option<actions::Command> {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        MouseEventKind::Moved => {
            if let Some(index) = state.activity_filter_row_at(column, row) {
                state.filter_row = index;
            }
            None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let index = state.activity_filter_row_at(column, row)?;
            state.filter_row = index;
            if index == SAVE_SELECTION {
                return Some(Command::SaveFilter.into());
            }
            Some(match index {
                0 => Command::ToggleFetch.into(),
                1 => Command::ToggleBuilds.into(),
                _ => Command::ToggleActivityHistory.into(),
            })
        }
        _ => None,
    }
}

fn filter_key(state: &mut State, key: KeyEvent) -> Option<actions::Command> {
    match key.code {
        KeyCode::Esc => Some(actions::Command::CloseOverlay),
        KeyCode::Up | KeyCode::Char('k') => {
            state.filter_row = state.filter_row.saturating_sub(1);
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.filter_row = (state.filter_row + 1).min(SAVE_SELECTION);
            None
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.filter_row == SAVE_SELECTION => {
            Some(Command::SaveFilter.into())
        }
        KeyCode::Char('s' | 'S') => Some(Command::SaveFilter.into()),
        KeyCode::Enter | KeyCode::Char(' ') => Some(match state.filter_row {
            0 => Command::ToggleFetch.into(),
            1 => Command::ToggleBuilds.into(),
            _ => Command::ToggleActivityHistory.into(),
        }),
        _ => None,
    }
}

fn frame_key(key: KeyEvent) -> Option<actions::Command> {
    match key.code {
        KeyCode::Char('a' | 'A') => Some(Command::ToggleActivity.into()),
        KeyCode::Char('t' | 'T') => Some(Command::OpenActivityFilter.into()),
        KeyCode::Char('v' | 'V') => Some(Command::ToggleActivityView.into()),
        _ => None,
    }
}

/// What one Nix activity event reports about the run's own line. A PlainMessage
/// carries text the run re-reads as its output.
#[derive(Debug, Clone)]
pub enum Fact {
    PlainMessage { id: u64, text: String },
    Started(Started),
    Progress { phase: String, name: String },
    Finish { id: u64, failed: bool },
}

/// Apply one decoded Nix activity event to the rows and report what the run's line must do.
/// Folds Nix's repeated announcements so each thing keeps one row.
pub fn apply_json(state: &mut State, value: &Value) -> Vec<Fact> {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return Vec::new();
    };
    let kind = value.get("type").and_then(Value::as_i64).unwrap_or(-1);
    let fields = value
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut facts = Vec::new();
    match action {
        "start" => {
            state.acts.insert(id, value.clone());
            state.note_set_start(id, kind, value);
            if kind == PLAIN_MESSAGE {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    facts.push(Fact::PlainMessage {
                        id,
                        text: text.to_owned(),
                    });
                }
            } else if matches!(kind, 100 | 101 | 108 | 109) {
                let parent_owner = value
                    .get("parent")
                    .and_then(Value::as_u64)
                    .and_then(|parent| state.aliases.get(&parent).copied());
                if let Some(owner) = parent_owner {
                    state.aliases.insert(id, owner);
                } else if kind != QUERY_PATH_INFO
                    && let Some((subject, name)) = transfer_subject(kind, &fields)
                {
                    // A narinfo is a cache being asked about a path: the row belongs to the cache.
                    if let Some(host) = probe_host(&subject) {
                        let cache = format!("probe:{host}");
                        let (owner, started) = state.claim_row(id, &cache, &host, "probe");
                        state.count_announcement(owner, &cache, &subject);
                        if let Some(started) = started {
                            facts.push(Fact::Started(started));
                        }
                    } else {
                        // Reuse the row its subject owns: one fetch, not one row per cache.
                        let (_, started) = state.claim_row(id, &subject, &name, "download");
                        if let Some(started) = started {
                            facts.push(Fact::Started(started));
                        }
                    }
                }
            } else if kind == 105
                && let Some(path) = fields.first().and_then(Value::as_str)
                && path.ends_with(".drv")
            {
                let started = state.start_activity(id, &store_name(path), "build");
                // The first build says fetching is over.
                state.finish_fetching();
                facts.push(Fact::Started(started));
            }
        }
        "result" => {
            if kind == 105 {
                state.note_set_progress(id, &fields);
                let owner = state.aliases.get(&id).copied().unwrap_or(id);
                if let Some(activity) = state.active.get_mut(&owner)
                    && activity.kind == ActivityKind::Download
                    && let Some(units) = fields.first().and_then(Value::as_u64)
                {
                    activity.units = activity.units.max(units);
                    // Expected size rides beside the moved bytes; zero means it stays unknown.
                    if let Some(expected) = fields.get(1).and_then(Value::as_u64) {
                        activity.total = activity.total.max(expected);
                    }
                    if units > 0 && activity.active_at.is_none() {
                        activity.active_at = Some(std::time::Instant::now());
                        activity.state = ActivityState::Running;
                    }
                }
            } else if kind == 101
                && let (Some(activity), Some(line)) = (
                    state.active.get_mut(&id),
                    fields.first().and_then(Value::as_str),
                )
                && let Some(phase) = line.strip_prefix("Running phase: ")
            {
                activity.phase = human_phase(phase);
                facts.push(Fact::Progress {
                    phase: activity.phase.clone(),
                    name: activity.name.clone(),
                });
            } else if kind == 104
                && let (Some(activity), Some(phase)) = (
                    state.active.get_mut(&id),
                    fields.first().and_then(Value::as_str),
                )
            {
                activity.phase = human_phase(phase);
                facts.push(Fact::Progress {
                    phase: activity.phase.clone(),
                    name: activity.name.clone(),
                });
            }
        }
        "stop" => {
            let owner = state.aliases.remove(&id).unwrap_or(id);
            // Aggregate rows have no `stop` of their own; the phase boundary closes them.
            let aggregate = state.active.get(&owner).is_some_and(|activity| {
                matches!(activity.kind, ActivityKind::Source | ActivityKind::Probe)
            });
            if owner == id && !aggregate {
                facts.push(Fact::Finish {
                    id: owner,
                    failed: false,
                });
            }
        }
        _ => {}
    }
    facts
}

fn human_phase(phase: &str) -> String {
    match phase {
        "unpackPhase" => "unpacking",
        "patchPhase" => "patching",
        "configurePhase" => "configuring",
        "buildPhase" | "compilePhase" => "compiling",
        "checkPhase" | "installCheckPhase" => "testing",
        "installPhase" => "installing",
        "fixupPhase" => "finishing",
        other => other.trim_end_matches("Phase"),
    }
    .to_owned()
}

#[cfg(test)]
mod tests;
