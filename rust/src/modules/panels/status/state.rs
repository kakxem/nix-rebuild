//! The run's state: what it rebuilds, how far it is, what failed, and whether it is stopping.

use std::time::{Duration, Instant};

use crate::common::nix_events::{LogLevel, NixEvent, store_name};
use crate::common::phase::Phase;
use crate::common::progress::StepProgress;
use crate::common::scope::Scope;

/// A derivation Nix could not build, read out of the error text: the build's own event carries no outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFailure {
    /// The derivation path, for the `nix log` command the prompt offers.
    pub drv: String,
    /// The store name of the derivation, which the Activity row carries.
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct State {
    pub scope: Scope,
    pub flake: String,
    pub action: String,
    pub phase: Phase,
    pub message: String,
    /// Shown beside the phase badge; separate from `status` so a warning cannot overwrite the outcome.
    pub notice: Option<String>,
    pub exit_code: i32,
    /// The exit code the child reported, once it has been reaped.
    pub child_exit_code: Option<i32>,
    /// What the failing step exited with; a derivation failure sets it too.
    pub step_exit_code: Option<i32>,
    pub step: Option<StepProgress>,
    /// The derivations Nix named as unbuildable, in order; a run that died before building has none.
    pub failures: Vec<BuildFailure>,
    /// The failing step's last stderr line. Dropped when the failure is recorded, so a line
    /// from earlier work can never be read as this failure's cause.
    pub failure_reason: Option<String>,
    pub dirty_tree: bool,
    pub cancel_requested: bool,
    pub started: Instant,
    pub finished_at: Option<Instant>,
    /// The elapsed time a reopened run shows, in place of a live clock.
    pub replay_elapsed: Option<Duration>,
    /// True while lines still belong to the error block a previous line opened.
    error_block: bool,
}

impl State {
    pub fn new(scope: Scope, flake: String, boot: bool) -> Self {
        Self {
            scope,
            flake,
            action: if boot { "boot" } else { "switch" }.into(),
            phase: Phase::Preparing,
            message: "starting rebuild".into(),
            notice: None,
            exit_code: 0,
            child_exit_code: None,
            step_exit_code: None,
            step: None,
            failures: Vec::new(),
            failure_reason: None,
            dirty_tree: false,
            cancel_requested: false,
            started: Instant::now(),
            finished_at: None,
            replay_elapsed: None,
            error_block: false,
        }
    }

    /// Apply what an Activity row's start reports: the run owns the phase badge and the
    /// status, the panel keeps its own rows.
    pub fn apply_started(&mut self, building: bool, status: String) {
        if building {
            self.phase = Phase::Building;
        }
        self.message = status;
    }

    pub fn elapsed(&self) -> Duration {
        // A reopened run is frozen at the length the run actually took.
        if let Some(elapsed) = self.replay_elapsed {
            return elapsed;
        }
        self.finished_at
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(self.started)
    }

    pub fn finish_timer(&mut self) {
        if self.finished_at.is_none() {
            self.finished_at = Some(Instant::now());
        }
    }

    pub fn note_plain_message(&mut self, text: &str) {
        self.phase = Phase::Evaluating;
        self.message = NixEvent::from_output(text).summary().to_owned();
    }

    pub fn note_progress(&mut self, summary: &str) {
        if !self.phase.is_running() {
            return;
        }
        if summary.contains("activating") || summary.contains("Activation") {
            self.phase = Phase::Activating;
            self.message = summary.to_owned();
        } else if summary.contains("copying") || summary.contains("downloading") {
            self.message = summary.to_owned();
        }
    }

    pub fn note_activity_progress(&mut self, phase: &str, name: &str) {
        self.message = format!("{phase} {name}");
    }

    /// The dirty-tree warning becomes the corner badge. Returns true so the caller drops the line.
    pub fn note_dirty_tree_warning(&mut self, event: &NixEvent) -> bool {
        if event.level == LogLevel::Warning && event.raw().to_ascii_lowercase().contains("is dirty")
        {
            self.dirty_tree = true;
            return true;
        }
        false
    }

    /// A blank line is what closes an error block.
    pub fn close_error_block(&mut self) {
        self.error_block = false;
    }

    /// Keep one error block at one level: only its first line carries `error:`, so indented
    /// lines and the prefixes [`is_error_continuation`] names are re-levelled to Error.
    pub fn settle_error_block(&mut self, event: NixEvent) -> NixEvent {
        let continuation = self.error_block
            && event.level != LogLevel::Error
            && is_error_continuation(event.raw());
        self.error_block = event.level == LogLevel::Error || continuation;
        if continuation {
            return event.with_level(LogLevel::Error);
        }
        event
    }

    /// Returns the name its rows carry when the failure is new, so the caller can mark that work.
    pub fn note_failure(&mut self, drv: &str) -> Option<String> {
        if self.failures.iter().any(|failure| failure.drv == drv) {
            return None;
        }
        let name = store_name(drv);
        self.failures.push(BuildFailure {
            drv: drv.to_owned(),
            name: name.clone(),
        });
        Some(name)
    }

    pub fn note_failed_derivation(&mut self, line: &str) -> Option<String> {
        let drv = failed_derivation(line)?;
        self.note_failure(&drv)
    }

    pub fn names_failure(&self, name: &str) -> bool {
        self.failures.iter().any(|failure| failure.name == name)
    }

    /// The failure the run stops at: the first derivation Nix named.
    pub fn first_failure(&self) -> Option<&BuildFailure> {
        self.failures.first()
    }

    /// Record a step's bad exit. The reason line is dropped here, so the work before the
    /// step can never be read as this failure's cause.
    pub fn note_step_failure(&mut self, exit_code: i32) {
        self.step_exit_code = Some(exit_code);
        self.failure_reason = None;
    }

    /// Keep the step's last stderr line: the reason when no derivation explains the failure.
    pub fn note_stderr(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        self.failure_reason = Some(line.trim_end().to_owned());
    }

    /// Whether the rebuild itself stopped. A cleanup that failed after a good activation
    /// is not this and keeps its Activity rows. The cleanup outcome is passed in.
    pub fn rebuild_failed(&self, cleanup_succeeded: Option<bool>) -> bool {
        self.phase == Phase::Failed && cleanup_succeeded != Some(false)
    }

    /// What to say when the rebuild finishes without cleanup. `finished` lives in the panel.
    pub fn completion_status(&self, finished: usize) -> String {
        if finished == 0 {
            "nothing to do — every path was already in the store".into()
        } else {
            format!("{finished} activities completed")
        }
    }

    /// A one-off note; the outcome stays in `status`/`phase` so a late warning cannot reopen it.
    pub fn push_status_note(&mut self, note: String) {
        self.notice = Some(note);
    }

    /// Reports whether the caller must put the confirmation in front of the user first.
    pub fn request_cancel(&self) -> bool {
        self.phase.is_running()
    }

    pub fn confirm_cancel(&mut self) {
        self.cancel_requested = true;
        self.phase = Phase::Cancelling;
        self.message = "stopping Nix safely".into();
    }
}

/// The derivation a `Cannot build '<path>'.` error names: the only line that says which
/// package stopped the run (the JSON event closing a build carries no outcome).
fn failed_derivation(line: &str) -> Option<String> {
    let rest = line.split_once("Cannot build '")?.1;
    let (path, _) = rest.split_once('\'')?;
    path.starts_with("/nix/store/").then(|| path.to_owned())
}

/// Whether a line continues the error block a previous line opened: indented, or one of
/// the prefixes Nix prints in the block. Only its first line carries `error:`.
fn is_error_continuation(line: &str) -> bool {
    if line.starts_with([' ', '\t']) {
        return true;
    }
    [
        "Reason:",
        "Output paths:",
        "Last ",
        ">",
        "For full logs, run:",
        "nix log ",
        "/nix/store/",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}
