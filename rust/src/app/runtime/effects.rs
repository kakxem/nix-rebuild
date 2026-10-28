//! The effects the reducer asks for, carried out where the side effects live.

use super::*;

/// Open the question about the journal, counting the runs it would delete.
pub(crate) fn ask_remove_runs(app: &mut App) {
    app.records.count = records::count();
    app.open_overlay(Overlay::RemoveRunsPrompt);
}

/// Cut the recorded runs, keeping the one a live frame is still writing into.
pub(crate) fn remove_runs(app: &mut App) {
    app.dismiss_overlay();
    records::remove_all_except(app.run_id.as_deref());
    app.refresh_records();
}

pub(crate) fn apply(app: &mut App, action: Action, effects: &mut Vec<Effect>) {
    effects.extend(reduce(app, action));
}

/// Carry out one effect. The reducer never does, so every side effect is reachable from here.
pub(crate) fn run_effect(
    app: &mut App,
    effect: Effect,
    cleanup_started: &mut bool,
    cleanup_control: &mut cleanup::CleanupControl,
    tx: &Sender<RuntimeEvent>,
    tools: &ToolPaths,
) {
    match effect {
        Effect::None => {}
        Effect::CopyLogs(payload) => {
            // Verify the copy before arming the confirmation, so the check mark never lies.
            match logs::copy_to_clipboard(&payload) {
                Ok(()) => {
                    reduce(app, Action::ClipboardFinished(Ok(())));
                }
                Err(error) => {
                    // The elevated frame cannot always reach the user's compositor: say why.
                    app.status.push_status_note(format!("copy failed: {error}"));
                    app.logs
                        .push_entry(crate::common::nix_events::NixEvent::from_output(&format!(
                            "error: could not copy the logs: {error}"
                        )));
                    reduce(app, Action::ClipboardFinished(Err(error.to_string())));
                }
            }
        }
        Effect::StartCleanup => {
            app.begin_cleanup_phase();
            // An explicit request starts a pass of its own; one already running is refused
            // before it reaches here, so a second answer cannot be swallowed.
            start_cleanup(app, tx, tools, cleanup_control);
            *cleanup_started = true;
        }
        Effect::RemoveRuns => remove_runs(app),
        Effect::AskRemoveRuns => ask_remove_runs(app),
        Effect::SaveSettings => {
            // A settings file that cannot be written must never break a run.
            app.settings.config.save();
        }
        // Both interruptions are applied by the loop below, which owns the child.
        Effect::InterruptActiveJob | Effect::ForceKillActiveJob => {
            app.status.cancel_requested = true;
        }
        Effect::Exit(code) => {
            app.status.exit_code = code;
            app.should_quit = true;
        }
    }
}

pub(crate) fn start_cleanup(
    app: &mut App,
    tx: &Sender<RuntimeEvent>,
    tools: &ToolPaths,
    control: &cleanup::CleanupControl,
) {
    app.overlay = Overlay::None;
    app.status.phase = Phase::Cleanup;
    app.status.message = "pruning generations and collecting garbage".into();
    // A cancelled pass latched its flag; the next one starts from a clean control.
    control.reset();
    cleanup::start(
        app.cleanup.scope,
        app.config.keep_generations,
        tx.clone(),
        tools.clone(),
        control.clone(),
    );
}
