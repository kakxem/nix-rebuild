//! Run identity and outcome, the card that draws it, and the dialogs that ask about it.

mod state;
mod view;

pub use state::{BuildFailure, State};
pub use view::{
    FAILURE_BANNER_ROWS, footer_control, render_confirm_cancel, render_details,
    render_failure_dialog, render_failure_prompt, render_summary,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::common::actions;
use crate::common::config::Settings;
use crate::common::panels::{Height, PanelSpec};
use crate::common::phase::Phase;
use crate::common::ui::HelpEntry;

/// The Run card is three rows and never a drag handle: it holds one run's summary.
pub fn panel_spec(visible: bool) -> PanelSpec {
    PanelSpec::new(Height::Fixed(RUN_ROWS), visible)
}

const RUN_ROWS: u16 = 3;

pub fn help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry {
            key: "S",
            label: "Status",
            command: Some(Command::ToggleStatus.into()),
        },
        HelpEntry {
            key: "D",
            label: "Details of this run, and the id it is recorded under",
            command: Some(Command::OpenDetails.into()),
        },
        HelpEntry {
            key: "Esc",
            label: "Cancel a running rebuild, close a finished one",
            command: None,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    ToggleStatus,
    OpenDetails,
    /// Only asks to stop; the confirmation dialog comes up first.
    RequestCancel,
    ConfirmCancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    OpenDetails,
    RequestCancel,
    /// The dialog stands down and the run is stopping.
    ConfirmCancelled,
}

/// Apply one Run command and report effects; the composition root carries them out.
pub fn reduce(settings: &mut Settings, command: Command) -> Vec<Effect> {
    match command {
        // Showing or hiding a card belongs to this frame only; the file moves from Settings.
        Command::ToggleStatus => {
            settings.show_status = !settings.show_status;
            vec![]
        }
        Command::OpenDetails => vec![Effect::OpenDetails],
        Command::RequestCancel => vec![Effect::RequestCancel],
        Command::ConfirmCancel => vec![Effect::ConfirmCancelled],
    }
}

/// Handles `ChildExited` and a failed copy; every other action is offered and ignored.
pub fn handle_action(state: &mut State, action: &actions::Action) -> Vec<Effect> {
    match action {
        actions::Action::ChildExited { code } => {
            state.child_exit_code = Some(*code);
        }
        actions::Action::ClipboardFinished(Err(message)) => {
            state.push_status_note(format!("could not copy logs: {message}"));
        }
        _ => {}
    }
    Vec::new()
}

/// The slice's keys on the frame. When `dialog_open`, the **Details** dialog owns every key.
pub fn key_command(state: &State, key: KeyEvent, dialog_open: bool) -> Option<actions::Command> {
    if dialog_open {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => Some(actions::Command::CloseOverlay),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::Char('s' | 'S') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Command::ToggleStatus.into())
            }
            KeyCode::Char('d' | 'D') => Some(Command::OpenDetails.into()),
            // First Ctrl+C asks, a second one leaves; a finished run quits like Enter.
            KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(
                        state.phase,
                        Phase::Complete | Phase::Failed | Phase::Cancelled
                    ) =>
            {
                Some(actions::Command::Quit)
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if state.cancel_requested {
                    Some(actions::Command::ForceQuit)
                } else {
                    Some(Command::RequestCancel.into())
                }
            }
            KeyCode::Esc if state.phase.is_running() => Some(Command::RequestCancel.into()),
            KeyCode::Esc | KeyCode::Enter
                if matches!(
                    state.phase,
                    Phase::Complete | Phase::Failed | Phase::Cancelled
                ) =>
            {
                Some(actions::Command::Quit)
            }
            _ => None,
        }
    }
}

/// The **ConfirmCancel** dialog's keys. Ctrl+C is the escape hatch: never trap the terminal.
pub fn cancel_key(_state: &State, key: KeyEvent) -> Option<actions::Command> {
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(Command::ConfirmCancel.into()),
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(actions::Command::CloseOverlay),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(actions::Command::ForceQuit)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
