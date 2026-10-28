//! The `Command`, `Action` and `Effect` envelopes every slice speaks.
//! `Command` names each slice's own command so dispatch needs one enum; the reduce that applies
//! them lives in `app::reduce`, where the frame belongs.

use crate::common::hit_targets::Click;
use crate::modules::panels::activity;
use crate::modules::panels::help;
use crate::modules::panels::logs;
use crate::modules::panels::settings;
use crate::modules::panels::status;
use crate::modules::runs::records;
use crate::modules::work::cleanup;

/// One variant per slice plus the frame's own; keyboard, mouse and Help all produce these.
/// Cross-slice dispatch goes through the constructors below, so no slice names a sibling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Logs(logs::Command),
    Activity(activity::Command),
    Status(status::Command),
    Cleanup(cleanup::Command),
    Help(help::Command),
    Settings(settings::Command),
    Records(records::Command),
    Quit,
    ForceQuit,
    CloseOverlay,
}

impl Command {
    /// The cancel chip on the Activity card sends what its own key sends, so the question comes
    /// up first and a click cannot cancel blind.
    pub fn request_cancel() -> Self {
        crate::modules::panels::status::Command::RequestCancel.into()
    }
}

impl From<logs::Command> for Command {
    fn from(command: logs::Command) -> Self {
        Command::Logs(command)
    }
}

impl From<activity::Command> for Command {
    fn from(command: activity::Command) -> Self {
        Command::Activity(command)
    }
}

impl From<status::Command> for Command {
    fn from(command: status::Command) -> Self {
        Command::Status(command)
    }
}

impl From<cleanup::Command> for Command {
    fn from(command: cleanup::Command) -> Self {
        Command::Cleanup(command)
    }
}

impl From<help::Command> for Command {
    fn from(command: help::Command) -> Self {
        Command::Help(command)
    }
}

impl From<settings::Command> for Command {
    fn from(command: settings::Command) -> Self {
        Command::Settings(command)
    }
}

impl From<records::Command> for Command {
    fn from(command: records::Command) -> Self {
        Command::Records(command)
    }
}

impl Click {
    pub fn command(self) -> Command {
        match self {
            Click::Command(command) => command,
            Click::Help => crate::modules::panels::help::Command::OpenHelp.into(),
        }
    }
}

/// Anything that changes the model. Domain events are offered to every slice (rule 5).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Command(Command),
    Output(String),
    NixJson(String),
    ChildExited { code: i32 },
    CleanupExited { success: bool },
    ClipboardFinished(Result<(), String>),
    Tick,
}

/// Work the runtime must carry out; the reducer only describes it and stays pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// "Handled, no work", distinct from an empty result for "not applicable".
    None,
    CopyLogs(String),
    /// Open the journal question; the runtime counts the runs before it is asked.
    AskRemoveRuns,
    /// Delete the recorded runs, keeping the one this frame is still writing.
    RemoveRuns,
    StartCleanup,
    InterruptActiveJob,
    ForceKillActiveJob,
    Exit(i32),
    SaveSettings,
}
