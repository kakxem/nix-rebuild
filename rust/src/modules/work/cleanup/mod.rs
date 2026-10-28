//! The cleanup slice: the prompt after a good rebuild, its commands and
//! effects, and the engine that prunes generations and collects garbage.

pub mod engine;
mod state;
mod view;

pub use engine::{CleanupControl, run, run_plain, start};
pub use state::{CLEANUP_STAGES, State};
pub use view::{render_cleanup_activity, render_cleanup_dialog};

use crossterm::event::{KeyCode, KeyEvent};

/// The cleanup rows in the Settings dialog: the policy, how many generations stay, and a pass now.
pub fn settings_entries() -> Vec<crate::common::config::SettingEntry> {
    use crate::common::config::SettingEntry;

    vec![
        SettingEntry::choice(
            "Cleanup",
            "Cleanup after a rebuild",
            |settings| match settings.cleanup {
                CleanupPolicy::Ask => "ask".into(),
                CleanupPolicy::Always => "always".into(),
                CleanupPolicy::Never => "never".into(),
            },
            |settings| {
                settings.cleanup = match settings.cleanup {
                    CleanupPolicy::Ask => CleanupPolicy::Always,
                    CleanupPolicy::Always => CleanupPolicy::Never,
                    CleanupPolicy::Never => CleanupPolicy::Ask,
                }
            },
        ),
        SettingEntry::number(
            "Cleanup",
            "Generations kept",
            |settings| settings.keep_generations,
            |settings, delta| {
                let step = if delta < 0 { -1 } else { 1 };
                settings.keep_generations =
                    (settings.keep_generations as i64 + step).clamp(1, 50) as usize;
            },
        ),
        // The button asks the question a finished rebuild asks; it never starts a pass by itself.
        SettingEntry::action("Cleanup", "Cleanup now", Command::AskCleanup.into()),
    ]
}

use crate::common::actions;
use crate::common::cleanup_policy::CleanupPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    SkipCleanup,
    StartCleanup,
    /// Open the same question a finished rebuild opens, without a run to settle.
    AskCleanup,
}

/// What the slice asks of the runtime; the model never spawns anything itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Skip,
    Start,
    Ask,
}

pub fn reduce(command: Command) -> Vec<Effect> {
    match command {
        Command::SkipCleanup => vec![Effect::Skip],
        Command::StartCleanup => vec![Effect::Start],
        Command::AskCleanup => vec![Effect::Ask],
    }
}

/// Handles the cleaner's exit; every other action is ignored.
pub fn handle_action(state: &mut State, action: &actions::Action) -> Vec<Effect> {
    if let actions::Action::CleanupExited { success } = action {
        state.succeeded = Some(*success);
        if *success {
            state.stage = Some(CLEANUP_STAGES.len());
        }
    }
    Vec::new()
}

/// While the prompt is open it answers to y/n and nothing else. Ctrl+C stays the way out of every
/// dialog, so a question that waits for an answer never traps the terminal.
pub fn key_command(key: KeyEvent, dialog_open: bool) -> Option<actions::Command> {
    if !dialog_open {
        return None;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') => Some(Command::StartCleanup.into()),
        KeyCode::Char('n' | 'N') | KeyCode::Esc | KeyCode::Enter => {
            Some(Command::SkipCleanup.into())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
