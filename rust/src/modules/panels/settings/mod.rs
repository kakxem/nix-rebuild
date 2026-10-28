//! The Settings dialog: the rows the slices contribute, in sections, driven by the frame.

use crossterm::event::{KeyCode, KeyEvent};

use crate::common::actions;
use crate::common::ui::{ControlSpec, HelpEntry};

mod state;
#[cfg(test)]
mod tests;
mod view;

pub use state::{Line, State};
pub use view::render_settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    ToggleSettings,
    MoveSelection(i64),
    ChangeValue(i64),
    /// Put the first drawn line here, from a click or drag on the card's bar.
    SetScroll(usize),
    SelectFirst,
    SelectLast,
}

/// What the row asks of the runtime: the file is written by the frame, not by the reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    SaveSettings,
    /// A button row, carried out by the root as the command its own slice would have sent.
    Run(actions::Command),
}

/// The dialog's footer control: the key that opens it, always lit like Help.
pub fn footer_control() -> ControlSpec {
    ControlSpec {
        command: Command::ToggleSettings.into(),
        prefix: "",
        key: "G",
        icon: Some('\u{f013}'),
        label: " Settings",
        active: true,
        enabled: true,
    }
}

pub fn help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry {
            key: "G",
            label: "Open the settings",
            command: Some(Command::ToggleSettings.into()),
        },
        HelpEntry {
            key: "\u{2190}/\u{2192}",
            label: "Change the selected setting",
            command: Some(Command::ChangeValue(1).into()),
        },
    ]
}

/// Apply one row command. The dialog's own toggle is the frame's (the root opens or closes it);
/// this slice only edits the file's copy.
pub fn reduce(state: &mut State, command: Command) -> Vec<Effect> {
    match command {
        Command::ToggleSettings => vec![],
        Command::MoveSelection(delta) => {
            state.move_selection(delta);
            vec![]
        }
        Command::ChangeValue(delta) => match state.selected() {
            // A row that is a button runs its command; the file keeps the value it had.
            Some(entry) => match entry.action_command() {
                Some(command) => vec![Effect::Run(command)],
                None => {
                    entry.apply(&mut state.config, delta);
                    vec![Effect::SaveSettings]
                }
            },
            None => vec![],
        },
        Command::SetScroll(offset) => {
            state.set_scroll(offset);
            vec![]
        }
        Command::SelectFirst => {
            state.select_first();
            vec![]
        }
        Command::SelectLast => {
            state.select_last();
            vec![]
        }
    }
}

/// Keys the dialog answers: its own toggle always, the rows only while it is up.
pub fn key_command(key: KeyEvent, dialog_open: bool) -> Option<actions::Command> {
    match key.code {
        KeyCode::Char('g' | 'G') => Some(Command::ToggleSettings.into()),
        _ if !dialog_open => None,
        KeyCode::Esc => Some(actions::Command::CloseOverlay),
        KeyCode::Up | KeyCode::Char('k') => Some(Command::MoveSelection(-1).into()),
        KeyCode::Down | KeyCode::Char('j') => Some(Command::MoveSelection(1).into()),
        KeyCode::Left => Some(Command::ChangeValue(-1).into()),
        KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
            Some(Command::ChangeValue(1).into())
        }
        KeyCode::Home => Some(Command::SelectFirst.into()),
        KeyCode::End => Some(Command::SelectLast.into()),
        _ => None,
    }
}

/// Moving over a row selects it; clicking one selects it and changes it, like a switch.
/// A click on a number's `-`/`+` steps that number without moving the selection.
pub fn mouse_command(
    state: &mut State,
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
) -> Option<actions::Command> {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        MouseEventKind::Moved => {
            if let Some(index) = state.row_at(column, row) {
                state.row = index;
            }
            None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(delta) = state.button_at(column, row) {
                return Some(Command::ChangeValue(delta).into());
            }
            let index = state.row_at(column, row)?;
            state.row = index;
            Some(Command::ChangeValue(1).into())
        }
        MouseEventKind::ScrollUp => Some(Command::MoveSelection(-3).into()),
        MouseEventKind::ScrollDown => Some(Command::MoveSelection(3).into()),
        _ => None,
    }
}
