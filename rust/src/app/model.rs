//! One field per slice, plus the glue that sequences slices when an event concerns several.

use std::time::Duration;

use serde_json::Value;

use crate::app::dashboard::{Heights, PanelId, Resize};
use crate::app::reduce::reduce;
use crate::common::actions::Action;
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::config::Settings;
use crate::common::hit_targets::{Click, ClickMap, Hover, ScrollbarDrag};
use crate::common::nix_events::NixEvent;
use crate::common::panels::PanelSpec;
use crate::common::phase::Phase;
use crate::common::scope::Scope;
use crate::modules::locks;
use crate::modules::panels::activity;
use crate::modules::panels::help;
use crate::modules::panels::logs;
use crate::modules::panels::settings;
use crate::modules::panels::status;
use crate::modules::runs::records;
use crate::modules::work::cleanup;

/// Which dialog is in front. When two slices want an overlay at once, the last one wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    ConfirmCancel,
    CleanupPrompt,
    Help,
    Details,
    LogFilter,
    ActivityFilter,
    /// The settings: the file's defaults, edited in a dialog of their own.
    Settings,
    /// The question before the recorded runs are deleted.
    RemoveRunsPrompt,
}

#[derive(Debug, Clone)]
pub struct App {
    pub status: status::State,
    /// How this run was invoked: `switch`, `boot`, `update` or `rollback`. The journal records
    /// it, because a multi-step run ends on a `switch` whatever started it.
    pub flow: String,
    /// Record id for this run; `None` until the record exists or if the run could not be recorded.
    pub run_id: Option<String>,
    /// The kept lock file this run replaced, so the journal can point at it.
    pub kept_lock: Option<String>,
    /// True while the closing frame (the one the user leaves) is being drawn.
    pub closing: bool,
    /// True when this frame reads back a recorded run rather than a live one.
    pub reopened: bool,
    /// What `config.toml` holds, as this frame reads it; the card edits a copy and saves it.
    pub config: Settings,
    pub overlay: Overlay,
    /// Workflow overlay hidden by a temporary dialog; closing that dialog restores this one.
    overlay_return: Overlay,
    pub should_quit: bool,
    pub activity: activity::State,
    pub logs: logs::State,
    /// The Settings dialog: its selection, its unsaved copy and where its rows landed.
    pub settings: settings::State,
    pub scrollbar_drag: Option<ScrollbarDrag>,
    /// Set by Ctrl+C: leave immediately, whatever dialog is open.
    pub force_quit: bool,
    pub hover: Option<Hover>,
    pub help: help::State,
    /// Clickable regions in absolute screen cells, refreshed by the renderer.
    pub clicks: ClickMap,
    /// Heights a border drag has pinned, for this frame only.
    pub heights: Heights,
    /// What the last render laid out: the content height and how each card sized itself. A new
    /// size or a card shown or hidden drops the pinned heights, so no row is left over.
    pub layout_key: Option<(u16, [PanelSpec; PanelId::ALL.len()])>,
    /// The border being dragged right now, if any.
    pub resizing: Option<Resize>,
    pub cleanup: cleanup::State,
    /// What the journal question needs: how many runs it would delete.
    pub records: records::State,
}

impl App {
    pub fn new(scope: Scope, flake: String, boot: bool, policy: CleanupPolicy) -> Self {
        let run = status::State::new(scope, flake, boot);
        Self {
            flow: run.action.clone(),
            status: run,
            run_id: None,
            kept_lock: None,
            closing: false,
            reopened: false,
            config: Settings::default(),
            overlay: Overlay::None,
            overlay_return: Overlay::None,
            should_quit: false,
            activity: activity::State::default(),
            logs: logs::State {
                copied_confirm: Duration::from_millis(1200),
                ..logs::State::default()
            },
            scrollbar_drag: None,
            force_quit: false,
            hover: None,
            // Order fixed here: panels top to bottom, then the run, the frame's Esc, the overlay's keys.
            help: help::State::with_entries(
                logs::help_entries()
                    .into_iter()
                    .chain(activity::help_entries())
                    .chain(settings::help_entries())
                    .chain(status::help_entries())
                    .chain(help::help_entries())
                    .collect(),
            ),
            settings: settings::State::with_entries(
                crate::app::dashboard::start_cards::entries()
                    .into_iter()
                    .chain(activity::settings_entries())
                    .chain(records::settings_entries())
                    .chain(locks::settings_entries())
                    .chain(cleanup::settings_entries())
                    .chain([records::exit_settings_entry()])
                    .collect(),
            ),
            clicks: ClickMap::default(),
            heights: Heights::default(),
            layout_key: None,
            resizing: None,
            cleanup: cleanup::State {
                policy,
                scope,
                ..cleanup::State::default()
            },
            records: records::State::default(),
        }
    }

    /// Start a run from these settings. Call sites apply command-line pins first, so a flag still wins.
    pub fn apply_settings(&mut self, settings: &Settings) {
        self.config = settings.clone();
        // The card edits the file's copy, so both start from the same values.
        self.settings.config = settings.clone();
        // A filter saved from a dialog is what this start reads, so the panel begins filtered.
        self.logs.filter = logs::LogFilter::from_saved(
            settings.log_filter_query.as_deref(),
            settings.log_filter_kinds.as_deref(),
        );
    }

    /// Measure the journal for the card's read-only rows; the runtime calls it while the card is up.
    pub fn refresh_records(&mut self) {
        let (runs, bytes) = records::measured();
        self.settings.recorded_runs = runs;
        self.settings.recorded_bytes = bytes;
    }

    /// Apply what an Activity row reports about the run's own line. The panel's facts are
    /// passed on separated, so the run never sees the panel's types.
    fn apply_started(&mut self, started: Option<activity::Started>) {
        if let Some(started) = started {
            self.status.apply_started(started.building, started.status);
        }
    }

    pub fn click_targets(&self) -> ClickMap {
        self.clicks.clone()
    }

    pub fn set_hover(&mut self, hover: Hover) {
        self.hover = Some(hover);
    }

    pub fn clear_hover(&mut self) {
        self.hover = None;
    }

    pub fn hovered_click(&self) -> Option<Click> {
        let hover = self.hover?;
        // The dialog in front owns the pointer, so the frame's own controls never light up under it.
        self.clicks.under(hover.column, hover.row)
    }

    /// Feed one line of the child's own output into the panes. A line can carry a whole
    /// error block, so it is split first; both channels then share one record per row.
    pub fn push_output(&mut self, line: &str) {
        for part in line.split('\n') {
            self.push_output_line(part);
        }
    }

    /// Offer one output line to every slice that listens (rule 5), in order: the run's
    /// phase first, then cleanup or run progress, the log entry last.
    fn push_output_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            // A blank line is what closes an error block.
            self.status.close_error_block();
            return;
        }
        let event = NixEvent::from_output(line);
        let summary = event.summary().to_owned();
        if self.status.phase == Phase::Cleanup {
            if let Some(status) = self.cleanup.note_output(&summary) {
                self.status.message = status.into();
            }
        } else {
            self.status.note_progress(&summary);
        }
        if self.status.note_dirty_tree_warning(&event) {
            return;
        }
        self.push_log_event(event);
    }

    fn push_log_event(&mut self, event: NixEvent) {
        let event = self.settle_error_block(event);
        self.logs.push_entry(event);
    }

    fn settle_error_block(&mut self, event: NixEvent) -> NixEvent {
        if let Some(name) = self.status.note_failed_derivation(event.raw()) {
            self.activity.mark_failure(&name);
        }
        self.status.settle_error_block(event)
    }

    /// Offer one line of the child's JSON event stream to every slice that listens (rule 5).
    pub fn ingest_json(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let action = value
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if action == "msg" {
            let message = value.get("msg").and_then(Value::as_str).unwrap_or_default();
            let level = value.get("level").and_then(Value::as_i64).unwrap_or(3);
            if !message.is_empty() {
                // One event can carry a whole error block, so split it like the child's own output.
                for part in message.split('\n') {
                    let event = NixEvent::from_message(part, level);
                    if self.status.note_dirty_tree_warning(&event) {
                        continue;
                    }
                    self.push_log_event(event);
                }
            }
            return;
        }
        for fact in activity::apply_json(&mut self.activity, &value) {
            match fact {
                activity::Fact::PlainMessage { id, text } => {
                    self.status.note_plain_message(&text);
                    self.push_output(&text);
                    if let Some(started) = self.activity.note_copy(id, &text) {
                        self.apply_started(Some(started));
                    }
                }
                activity::Fact::Started(started) => self.apply_started(Some(started)),
                activity::Fact::Progress { phase, name } => {
                    self.status.note_activity_progress(&phase, &name);
                }
                activity::Fact::Finish { id, failed } => self.finish_activity(id, failed),
            }
        }
    }

    /// Close one row at the run's outcome. A row asked to close as done may still be a
    /// failure, so the run is asked before the panel closes it.
    pub fn finish_activity(&mut self, id: u64, failed: bool) {
        let state = if failed || self.named_failure(id) {
            activity::ActivityState::Failed
        } else {
            activity::ActivityState::Done
        };
        self.activity.close_activity(id, state);
    }

    /// Whether Nix named this row as a derivation it could not build. The `stop` event
    /// carries no outcome and arrives before the error, so `done` rows are corrected here.
    fn named_failure(&self, id: u64) -> bool {
        self.activity.active.get(&id).is_some_and(|activity| {
            activity.kind == activity::ActivityKind::Build
                && self.status.names_failure(&activity.name)
        })
    }

    /// Close the rows still open once the child exits. A named failure leaves the leftovers
    /// NotBuilt (they are what it stopped); with nothing named, they carry the failure.
    pub fn finish_after_exit(&mut self, failed: bool) {
        if !failed {
            self.finish_all(false);
            return;
        }
        let named = self.status.first_failure().is_some();
        let ids: Vec<_> = self.activity.active.keys().copied().collect();
        for id in ids {
            if named {
                self.activity
                    .close_activity(id, activity::ActivityState::NotBuilt);
            } else {
                self.finish_activity(id, true);
            }
        }
    }

    pub fn finish_all(&mut self, failed: bool) {
        let ids: Vec<_> = self.activity.active.keys().copied().collect();
        for id in ids {
            self.finish_activity(id, failed);
        }
    }

    /// Ask the run to stop; if it can stop, the frame opens the confirmation dialog.
    pub fn request_cancel(&mut self) {
        if self.status.request_cancel() {
            self.overlay = Overlay::ConfirmCancel;
        }
    }

    pub fn confirm_cancel(&mut self) {
        self.overlay = Overlay::None;
        self.status.confirm_cancel();
    }

    /// Settle a confirmed cancel. The frame calls it on every pass while the run says
    /// it is stopping, so a second answer can never leave `cancelling` on screen.
    pub fn settle_cancel(&mut self, cleanup_started: bool) {
        if cleanup_started {
            self.status.message = "cleanup cancelled; rebuild activation remains in place".into();
        } else {
            self.finish_all(true);
            self.status.message = "rebuild cancelled; activation state may have changed".into();
        }
        self.status.phase = Phase::Cancelled;
        self.status.exit_code = 130;
    }

    pub fn open_overlay(&mut self, overlay: Overlay) {
        if self.overlay_return == Overlay::None && self.overlay != Overlay::None {
            self.overlay_return = self.overlay;
        }
        self.overlay = overlay;
    }

    pub fn dismiss_overlay(&mut self) {
        self.help.clear_help_query();
        self.overlay = std::mem::replace(&mut self.overlay_return, Overlay::None);
    }

    /// Mark the rebuild complete while the cleanup decision is pending. Temporary dialogs
    /// may cover the prompt but cannot move the run back into an evaluating state.
    pub fn request_cleanup_decision(&mut self) {
        self.status.phase = Phase::Complete;
        self.overlay = Overlay::CleanupPrompt;
        self.overlay_return = Overlay::None;
        self.status.message = "rebuild complete; cleanup decision required".into();
    }

    /// Leave cleanup undone and finish at the rebuild's own outcome. A question asked from the
    /// dialog has no run to settle, so only a run waiting on the answer gets its status back.
    pub fn skip_cleanup(&mut self) {
        // Declining puts back whatever the question covered: the dialog that asked it, or nothing.
        self.dismiss_overlay();
        if self.status.phase == Phase::Complete {
            self.status.message = self.completion_status();
        }
    }

    /// Ask the cleanup question without touching the run: the card's button asks the same thing.
    pub fn ask_cleanup(&mut self) {
        self.open_overlay(Overlay::CleanupPrompt);
    }

    /// Enter the cleanup phase; the runtime notices and starts the work, the model spawns nothing.
    pub fn begin_cleanup_phase(&mut self) {
        self.overlay = Overlay::None;
        self.overlay_return = Overlay::None;
        self.status.phase = Phase::Cleanup;
        self.cleanup.begin();
        self.status.message = "pruning generations and collecting garbage".into();
    }

    /// Settle the run at the cleanup's outcome. A cancelled run keeps its cancellation:
    /// a late result from a stopped pass must not resurrect it.
    pub fn finish_cleanup(&mut self, success: bool) {
        if self.status.phase == Phase::Cancelled {
            return;
        }
        self.overlay = Overlay::None;
        self.status.phase = if success {
            Phase::Complete
        } else {
            Phase::Failed
        };
        self.status.message = if success {
            "rebuild and cleanup completed".into()
        } else {
            "rebuild completed, cleanup failed".into()
        };
        self.status.exit_code = if success { 0 } else { 1 };
    }

    /// What to say when the rebuild finishes without cleanup; the panel's finished-row count is passed in.
    pub fn completion_status(&self) -> String {
        self.status.completion_status(self.activity.finished.len())
    }

    /// Whether the rebuild itself stopped, as opposed to a cleanup that failed after a
    /// good activation. The cleanup outcome is passed in; the run does not own it.
    pub fn rebuild_failed(&self) -> bool {
        self.status.rebuild_failed(self.cleanup.succeeded)
    }

    /// Rebuild a recorded session's dashboard: feed its events back through the live
    /// actions, then settle the run where the recording says it ended.
    pub fn reopen(
        meta: &crate::common::records::Meta,
        records: &[crate::common::records::Record],
    ) -> Self {
        let mut app = App::new(
            meta.scope(),
            meta.flake.clone(),
            meta.action == "boot",
            meta.policy(),
        );
        app.status.action = meta.action.clone();
        app.flow = meta.action.clone();
        app.run_id = Some(meta.id.clone());
        for record in records {
            let action = if record.json {
                Action::NixJson(record.line.clone())
            } else {
                Action::Output(record.line.clone())
            };
            // The effects are dropped: a recorded run has nothing left to run.
            let _ = reduce(&mut app, action);
        }
        // The replay drives phase, status and activities as if live, so finish_after_exit
        // closes leftover rows the way the live run does when the child exits.
        app.finish_after_exit(meta.exit_code != Some(0));
        app.status.finish_timer();
        app.status.exit_code = meta.exit_code.unwrap_or(0);
        app.status.message = meta.status.clone();
        app.status.replay_elapsed = meta.elapsed();
        app.status.phase = match meta.exit_code {
            Some(0) => Phase::Complete,
            Some(130) => Phase::Cancelled,
            Some(_) => Phase::Failed,
            None => Phase::Cancelled,
        };
        // Restore the failure banner: the code the run ended with, and whether the
        // cleanup is what failed, which is a different outcome that keeps its rows.
        app.cleanup.succeeded = meta.cleanup_ok;
        if app.status.phase == Phase::Failed {
            app.status.step_exit_code = meta.exit_code;
        }
        app
    }
}

#[cfg(test)]
mod tests {
    use crate::app::testing::app;

    #[test]
    fn remembered_settings_fill_a_new_run() {
        let mut app = app();
        let settings = crate::common::config::Settings {
            raw_logs: true,
            show_downloads: false,
            show_cleanup_progress: false,
            keep_generations: 9,
            ..Default::default()
        };

        app.apply_settings(&settings);

        assert!(app.config.raw_logs);
        assert!(!app.config.show_downloads);
        assert!(!app.config.show_cleanup_progress);
        assert_eq!(app.config.keep_generations, 9);
    }
}
