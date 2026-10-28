//! The keyboard help overlay: the entries it documents, the `/` search that narrows them, and the selection Enter dispatches.
//! Its rows come from every slice that owns a key; the composition root concatenates the lists, so this module never names a sibling.

mod state;
mod view;

pub use state::State;
pub use view::{footer_control, render_help};

use crossterm::event::{KeyCode, KeyEvent};

use crate::common::actions;
use crate::common::ui::HelpEntry;

/// The rows this slice owns: the pointer and the frame's own overlay key.
pub fn help_entries() -> Vec<HelpEntry> {
    vec![
        HelpEntry {
            key: "Mouse",
            label: "Wheel panels; click or drag scrollbars",
            command: None,
        },
        HelpEntry {
            key: "F2",
            label: "Release the mouse so text can be selected",
            command: None,
        },
        HelpEntry {
            key: "H",
            label: "Toggle this help",
            command: None,
        },
    ]
}

/// The help overlay's commands; running a row dispatches that row's own command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    OpenHelp,
    MoveSelection(i64),
    SelectFirst,
    SelectLast,
    StartQuery,
    /// Apply the pending search.
    EndQuery,
    QueryChar(char),
    QueryBackspace,
    ClearQuery,
}

/// What the overlay asks of the composition root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Open,
}

/// Apply one command that concerns the help overlay and report its effects.
pub fn reduce(state: &mut State, command: Command) -> Vec<Effect> {
    match command {
        Command::OpenHelp => vec![Effect::Open],
        Command::MoveSelection(delta) => {
            state.move_help_selection(delta);
            vec![]
        }
        Command::SelectFirst => {
            state.index = 0;
            vec![]
        }
        Command::SelectLast => {
            state.index = state.help_entries().count().saturating_sub(1);
            vec![]
        }
        Command::StartQuery => {
            state.filtering = true;
            vec![]
        }
        Command::EndQuery => {
            state.filtering = false;
            vec![]
        }
        Command::QueryChar(c) => {
            state.query.push(c);
            state.index = 0;
            vec![]
        }
        Command::QueryBackspace => {
            state.query.pop();
            state.index = 0;
            vec![]
        }
        Command::ClearQuery => {
            state.clear_help_query();
            vec![]
        }
    }
}

/// No event concerns this overlay: every event is offered and ignored (rule 5).
pub fn handle_action(_state: &mut State, _action: &actions::Action) -> Vec<Effect> {
    Vec::new()
}

/// Enter runs the selected row, so the list is a menu and not only a legend.
pub fn key_command(
    state: &mut State,
    key: KeyEvent,
    dialog_open: bool,
) -> Option<actions::Command> {
    if dialog_open {
        help_key(state, key)
    } else {
        match key.code {
            KeyCode::Char('h' | 'H') => Some(Command::OpenHelp.into()),
            _ => None,
        }
    }
}

/// Pointer handling: moving over a row selects it, a left click selects and runs it like a menu row.
pub fn mouse_command(
    state: &mut State,
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
) -> Option<actions::Command> {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        MouseEventKind::Moved => {
            if let Some(index) = state.help_row_at(column, row) {
                state.index = index;
            }
            None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let index = state.help_row_at(column, row)?;
            state.index = index;
            state.selected_help_command()
        }
        MouseEventKind::ScrollUp => Some(Command::MoveSelection(-3).into()),
        MouseEventKind::ScrollDown => Some(Command::MoveSelection(3).into()),
        _ => None,
    }
}

fn help_key(state: &mut State, key: KeyEvent) -> Option<actions::Command> {
    if state.filtering {
        match key.code {
            KeyCode::Esc => Some(Command::ClearQuery.into()),
            KeyCode::Enter => Some(Command::EndQuery.into()),
            KeyCode::Backspace => Some(Command::QueryBackspace.into()),
            KeyCode::Char(c) => Some(Command::QueryChar(c).into()),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::Esc | KeyCode::Char('h' | 'H') => Some(actions::Command::CloseOverlay),
            KeyCode::Char('/') => Some(Command::StartQuery.into()),
            KeyCode::Enter => state.selected_help_command(),
            KeyCode::Up | KeyCode::Char('k') => Some(Command::MoveSelection(-1).into()),
            KeyCode::Down | KeyCode::Char('j') => Some(Command::MoveSelection(1).into()),
            KeyCode::Home => Some(Command::SelectFirst.into()),
            KeyCode::End => Some(Command::SelectLast.into()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
