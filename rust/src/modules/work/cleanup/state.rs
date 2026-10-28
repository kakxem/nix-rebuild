//! The cleanup's state: pass progress, generations and freed space.

use std::time::Instant;

use crate::common::nix_events::cleanup_tally;

use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::scope::Scope;

/// Stages in the order the cleaner runs them. The strings are exact UI text:
/// the Activity card lists them and tests match them verbatim.
pub const CLEANUP_STAGES: [&str; 4] = [
    "Removing old generations",
    "Finding unreachable store paths",
    "Deleting unreachable store paths",
    "Optimizing store links",
];

#[derive(Debug, Clone)]
pub struct State {
    /// The run's pruning decision; the settings file only seeds it.
    pub policy: CleanupPolicy,
    /// What the pass prunes. An update activates both profiles, so it cleans both,
    /// while the Run card keeps naming the step that finished.
    pub scope: Scope,
    pub succeeded: Option<bool>,
    /// Index of the running stage; `Some(CLEANUP_STAGES.len())` means all done.
    pub stage: Option<usize>,
    pub generations: Vec<String>,
    /// Store paths deleted and the size the collector reported.
    pub freed: Option<(u64, String)>,
    pub started_at: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            policy: CleanupPolicy::default(),
            scope: Scope::Home,
            succeeded: None,
            stage: None,
            generations: Vec::new(),
            freed: None,
            started_at: None,
        }
    }
}

impl State {
    /// Parse one cleaner output line into stage, generations and freed tally.
    /// Returns the stage label when the line named one, for the run's status.
    pub fn note_output(&mut self, summary: &str) -> Option<&'static str> {
        let mut status = None;
        if let Some(index) = cleanup_stage_index(summary) {
            self.stage = Some(index);
            status = Some(CLEANUP_STAGES[index]);
        }
        if let Some(generations) = cleanup_generations(summary) {
            self.generations = generations;
        }
        if let Some(tally) = cleanup_tally(summary) {
            self.freed = Some(tally);
        }
        status
    }

    pub fn begin(&mut self) {
        self.started_at = Some(Instant::now());
    }
}

fn cleanup_stage_index(line: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("removing generations") || lower.starts_with("removing generation") {
        Some(0)
    } else if lower.contains("collecting unreachable")
        || lower.starts_with("finding garbage collector roots")
        || lower.starts_with("removing stale")
    {
        Some(1)
    } else if lower.starts_with("deleting garbage") || lower.starts_with("deleting '") {
        Some(2)
    } else if lower.contains("hard linking")
        || lower.contains("optimis")
        || lower.contains("optimiz")
    {
        Some(3)
    } else {
        None
    }
}

/// The ids the cleaner announced, so the view can name what is removed.
fn cleanup_generations(line: &str) -> Option<Vec<String>> {
    let list = line.strip_prefix("cleanup: removing generations ")?;
    let ids: Vec<String> = list
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    (!ids.is_empty()).then_some(ids)
}
