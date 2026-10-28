//! The Logs panel: the run's records, where the window looks, the filter, and the copy control.
//! Owns its commands, effects, reduce, keys, and its part of the world's events.

mod state;
mod view;

/// The filter dialog's rows: where its save button sits on screen and in the selection.
pub use state::{FILTER_ROWS, SAVE_ROW, SAVE_SELECTION};
pub use state::{FILTERABLE_KINDS, KindSet, LogCursor, LogEntry, LogFilter, State};
pub use view::{
    failure_copy_button, footer_control, log_rows, logs_control_specs, logs_text,
    render_log_filter, render_logs,
};

use std::process::{Command as ProcessCommand, Stdio};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::common::actions;
use crate::common::config::Settings;
use crate::common::panels::{Height, PanelSpec};
use crate::common::ui::HelpEntry;

/// The Logs card asks for two fifths of the frame, between seven and twenty-six rows.
pub fn panel_spec(visible: bool) -> PanelSpec {
    PanelSpec::new(
        Height::Fraction {
            num: 2,
            den: 5,
            min: 7,
            max: 26,
        },
        visible,
    )
}

/// Copy the log text via `wl-copy` or `xclip`, falling back between them: both Wayland and X11 run on this host.
pub fn copy_to_clipboard(payload: &str) -> anyhow::Result<()> {
    use std::io::Write as _;

    let candidates = clipboard_commands();
    let mut last_error = None;

    for (program, _kind) in candidates {
        let mut command = ProcessCommand::new(&program);
        if program.ends_with("xclip") {
            command.arg("-selection").arg("clipboard");
        }
        match command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(payload.as_bytes())?;
                }
                let status = child.wait()?;
                if status.success() {
                    return Ok(());
                }
                last_error = Some(format!("{program} exited with {status}"));
            }
            Err(error) => last_error = Some(format!("{program}: {error}")),
        }
    }

    anyhow::bail!(
        "{}",
        last_error.unwrap_or_else(|| "no clipboard tool available".into())
    )
}

fn clipboard_commands() -> Vec<(String, &'static str)> {
    if let Some(path) = std::env::var_os("NIX_REBUILD_CLIPBOARD") {
        let path = path.to_string_lossy().into_owned();
        let kind = if path.ends_with("xclip") {
            "x11"
        } else {
            "wayland"
        };
        return vec![(path, kind)];
    }
    vec![
        ("wl-copy".to_owned(), "wayland"),
        ("xclip".to_owned(), "x11"),
    ]
}

// The card holds no Logs rows: its only option, the raw lines, is a frame key and a toggle in
// the filter dialog, and the rows about recorded runs belong to the runs group.

/// The Logs panel's rows in the keyboard help overlay, owned here so the overlay never names this slice.
pub fn help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry {
            key: "L",
            label: "Logs",
            command: Some(Command::ToggleLogs.into()),
        },
        HelpEntry {
            key: "Ctrl+R",
            label: "Raw logs",
            command: Some(Command::ToggleRaw.into()),
        },
        HelpEntry {
            key: "F",
            label: "Filter logs",
            command: Some(Command::OpenLogFilter.into()),
        },
        HelpEntry {
            key: "Up",
            label: "Scroll logs up",
            command: Some(Command::ScrollLogs(3).into()),
        },
        HelpEntry {
            key: "Down",
            label: "Scroll logs down",
            command: Some(Command::ScrollLogs(-3).into()),
        },
        HelpEntry {
            key: "End",
            label: "Back to the newest log line",
            command: Some(Command::FollowLatest.into()),
        },
        HelpEntry {
            key: "C",
            label: "Copy every log line",
            command: Some(Command::CopyLogs.into()),
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    ToggleLogs,
    ToggleRaw,
    OpenLogFilter,
    ScrollLogs(i16),
    SetLogScroll(usize),
    PageLogsUp,
    PageLogsDown,
    FollowLatest,
    CopyLogs,
    /// Keep the current query and kinds as the filter a start begins with.
    SaveFilter,
}

/// What the panel asks of the runtime once its state is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    OpenFilter,
    CopyLogs(String),
    /// Hand the filter to the file's copy; the runtime writes it.
    SaveFilter {
        query: String,
        kinds: String,
    },
}

/// Apply one command that concerns the Logs panel and report its effects.
pub fn reduce(state: &mut State, settings: &mut Settings, command: Command) -> Vec<Effect> {
    match command {
        // Showing the card or the raw lines belongs to this frame only; the file moves from Settings.
        Command::ToggleLogs => {
            settings.show_logs = !settings.show_logs;
            vec![]
        }
        Command::ToggleRaw => {
            settings.raw_logs = !settings.raw_logs;
            state.saved = false;
            vec![]
        }
        Command::OpenLogFilter => {
            state.filter_row = 0;
            state.filter_editing = false;
            state.saved = false;
            vec![Effect::OpenFilter]
        }
        Command::ScrollLogs(lines) => {
            state.scroll_logs_by(lines as i64);
            vec![]
        }
        Command::SetLogScroll(offset) => {
            state.set_log_scroll(offset);
            vec![]
        }
        Command::PageLogsUp => {
            state.scroll_logs_by(state.viewport.max(1) as i64);
            vec![]
        }
        Command::PageLogsDown => {
            state.scroll_logs_by(-(state.viewport.max(1) as i64));
            vec![]
        }
        Command::FollowLatest => {
            state.follow_latest_logs();
            vec![]
        }
        Command::CopyLogs => vec![Effect::CopyLogs(state.logs_for_clipboard())],
        Command::SaveFilter => {
            let (query, kinds) = state.filter.saved();
            state.saved = true;
            vec![Effect::SaveFilter { query, kinds }]
        }
    }
}

/// The copy confirmation's life cycle; every other event is offered and ignored (rule 5).
pub fn handle_action(state: &mut State, action: &actions::Action) -> Vec<Effect> {
    match action {
        actions::Action::ClipboardFinished(Ok(())) => {
            state.arm_copied_marker(state.copied_confirm);
        }
        actions::Action::ClipboardFinished(Err(_)) => {
            state.clear_copied_marker();
        }
        actions::Action::Tick if state.copied_confirmation_expired() => {
            state.clear_copied_marker();
        }
        _ => {}
    }
    Vec::new()
}

/// The handler may edit the filter query before naming a command.
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

/// Pointer handling while the filter dialog is open.
pub fn mouse_command(
    state: &mut State,
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
) -> Option<actions::Command> {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        MouseEventKind::Moved => {
            if let Some(index) = state.filter_row_at(column, row) {
                state.filter_row = index;
            }
            None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let index = state.filter_row_at(column, row)?;
            state.filter_row = index;
            if index == 0 {
                state.filter_editing = true;
            } else if index == 1 {
                return Some(Command::ToggleRaw.into());
            } else if index == SAVE_SELECTION {
                return Some(Command::SaveFilter.into());
            } else if let Some(kind) = FILTERABLE_KINDS.get(index - 2) {
                state.filter.kinds.toggle(*kind);
                state.saved = false;
                state.clamp_log_scroll();
            }
            None
        }
        _ => None,
    }
}

/// Query keys and navigation keys are told apart by whether the query field has focus.
fn filter_key(state: &mut State, key: KeyEvent) -> Option<actions::Command> {
    if state.filter_editing {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                state.filter_editing = false;
                return None;
            }
            KeyCode::Backspace => {
                state.filter.query.pop();
                state.saved = false;
                state.clamp_log_scroll();
                return None;
            }
            KeyCode::Char(c) => {
                state.filter.query.push(c);
                state.saved = false;
                state.clamp_log_scroll();
                return None;
            }
            _ => {}
        }
        return None;
    }

    let rows = FILTERABLE_KINDS.len() + 3; // query, raw mode, semantic kinds, then the save button
    match key.code {
        KeyCode::Esc => Some(actions::Command::CloseOverlay),
        KeyCode::Up | KeyCode::Char('k') => {
            state.filter_row = state.filter_row.saturating_sub(1);
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.filter_row = (state.filter_row + 1).min(rows - 1);
            None
        }
        KeyCode::Enter if state.filter_row == 0 => {
            state.filter_editing = true;
            None
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.filter_row == 1 => {
            Some(Command::ToggleRaw.into())
        }
        // The last row is the button that keeps this filter for the next start.
        KeyCode::Enter | KeyCode::Char(' ') if state.filter_row == SAVE_SELECTION => {
            Some(Command::SaveFilter.into())
        }
        KeyCode::Char('s' | 'S') => Some(Command::SaveFilter.into()),
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(kind) = state
                .filter_row
                .checked_sub(2)
                .and_then(|row| FILTERABLE_KINDS.get(row))
            {
                state.filter.kinds.toggle(*kind);
                state.saved = false;
                state.clamp_log_scroll();
            }
            None
        }
        KeyCode::Char('i') if state.filter_row == 0 => {
            state.filter_editing = true;
            None
        }
        KeyCode::Char('c' | 'C') => {
            state.filter.clear();
            state.saved = false;
            state.clamp_log_scroll();
            None
        }
        _ => None,
    }
}

fn frame_key(key: KeyEvent) -> Option<actions::Command> {
    match key.code {
        KeyCode::Char('l' | 'L') => Some(Command::ToggleLogs.into()),
        KeyCode::Char('r' | 'R') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Command::ToggleRaw.into())
        }
        KeyCode::Char('f' | 'F') => Some(Command::OpenLogFilter.into()),
        KeyCode::Char('c' | 'C') => Some(Command::CopyLogs.into()),
        KeyCode::Up => Some(Command::ScrollLogs(1).into()),
        KeyCode::Down => Some(Command::ScrollLogs(-1).into()),
        KeyCode::PageUp => Some(Command::PageLogsUp.into()),
        KeyCode::PageDown => Some(Command::PageLogsDown.into()),
        KeyCode::End => Some(Command::FollowLatest.into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
