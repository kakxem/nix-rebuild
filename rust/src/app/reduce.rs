//! The root reduce: one command or one world event, applied to every slice that listens.
//! Rendering never mutates the model; the reducer never performs I/O, it returns [`Effect`]s.

use crate::app::model::{App, Overlay};
use crate::common::actions::{Action, Command, Effect};
use crate::common::phase::Phase;
use crate::modules::panels::activity;
use crate::modules::panels::help;
use crate::modules::panels::logs;
use crate::modules::panels::settings;
use crate::modules::panels::status;
use crate::modules::runs::records;
use crate::modules::work::cleanup;

pub fn reduce(model: &mut App, action: Action) -> Vec<Effect> {
    if let Action::Command(command) = action {
        return apply_command(model, command);
    }
    match &action {
        Action::Output(line) => model.push_output(line),
        Action::NixJson(line) => model.ingest_json(line),
        Action::CleanupExited { success } => model.finish_cleanup(*success),
        _ => {}
    }
    // Rule 5: a world event fans out to every slice and the root concatenates the effects.
    // `Output` and `NixJson` fan out from the model's ingest glue instead.
    let mut effects = Vec::new();
    for handled in status::handle_action(&mut model.status, &action) {
        effects.extend(run_effect(model, handled));
    }
    for handled in cleanup::handle_action(&mut model.cleanup, &action) {
        effects.extend(cleanup_effect(model, handled));
    }
    for handled in activity::handle_action(&mut model.activity, &action) {
        effects.extend(activity_effect(model, handled));
    }
    for handled in logs::handle_action(&mut model.logs, &action) {
        effects.extend(logs_effect(model, handled));
    }
    if effects.is_empty() {
        vec![Effect::None]
    } else {
        effects
    }
}

fn apply_command(model: &mut App, command: Command) -> Vec<Effect> {
    let effects = dispatch_command(model, command);
    // Running a Help row acts and then closes the dialog; driving the list is not running a row.
    if model.overlay == Overlay::Help && !matches!(command, Command::Help(_)) {
        model.dismiss_overlay();
    }
    effects
}

fn dispatch_command(model: &mut App, command: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    match command {
        // Toggles are settings: the runtime writes them back, the reducer never touches the disk.
        Command::Logs(command) => {
            for effect in logs::reduce(&mut model.logs, &mut model.config, command) {
                effects.extend(logs_effect(model, effect));
            }
        }
        Command::Activity(command) => {
            for effect in activity::reduce(&mut model.activity, &mut model.config, command) {
                effects.extend(activity_effect(model, effect));
            }
        }
        Command::Status(command) => {
            for effect in status::reduce(&mut model.config, command) {
                effects.extend(run_effect(model, effect));
            }
        }
        Command::Settings(command) => {
            if matches!(command, settings::Command::ToggleSettings) {
                // The key opens the dialog; on the dialog itself it closes it. What the next
                // start reads is the dialog's own rows, which write the file.
                if model.overlay == Overlay::Settings {
                    model.dismiss_overlay();
                } else {
                    model.open_overlay(Overlay::Settings);
                }
                effects.push(Effect::None);
            } else {
                for effect in settings::reduce(&mut model.settings, command) {
                    effects.extend(settings_effect(model, effect));
                }
            }
        }
        Command::Cleanup(command) => {
            // The card's button must not ask or start a pass under a rebuild that is still working.
            if matches!(
                command,
                cleanup::Command::StartCleanup | cleanup::Command::AskCleanup
            ) && model.status.phase.is_working()
            {
                return vec![Effect::None];
            }
            for effect in cleanup::reduce(command) {
                effects.extend(cleanup_effect(model, effect));
            }
        }
        Command::Records(command) => {
            for effect in records::reduce(command) {
                effects.extend(records_effect(effect));
            }
        }
        Command::Help(command) => {
            for effect in help::reduce(&mut model.help, command) {
                effects.extend(help_effect(model, effect));
            }
        }
        Command::Quit => {
            model.should_quit = true;
            effects.push(Effect::Exit(model.status.exit_code));
        }
        Command::ForceQuit => {
            model.force_quit = true;
            model.should_quit = true;
            model.overlay = Overlay::None;
            model.status.exit_code = 130;
            model.finish_all(true);
            model.status.phase = Phase::Cancelled;
            effects.push(Effect::ForceKillActiveJob);
            effects.push(Effect::Exit(130));
        }
        Command::CloseOverlay => {
            model.dismiss_overlay();
            effects.push(Effect::None);
        }
    }
    if effects.is_empty() {
        vec![Effect::None]
    } else {
        effects
    }
}

fn logs_effect(model: &mut App, effect: logs::Effect) -> Vec<Effect> {
    match effect {
        logs::Effect::OpenFilter => {
            model.open_overlay(Overlay::LogFilter);
            vec![Effect::None]
        }
        logs::Effect::CopyLogs(payload) => vec![Effect::CopyLogs(payload)],
        // The dialog writes the file's copy of the filter; the frame's own filter stays live.
        logs::Effect::SaveFilter { query, kinds } => {
            model.settings.config.log_filter_query = Some(query);
            model.settings.config.log_filter_kinds = Some(kinds);
            vec![Effect::SaveSettings]
        }
    }
}

fn records_effect(effect: records::Effect) -> Vec<Effect> {
    match effect {
        records::Effect::AskRemove => vec![Effect::AskRemoveRuns],
        records::Effect::Remove => vec![Effect::RemoveRuns],
    }
}

fn activity_effect(model: &mut App, effect: activity::Effect) -> Vec<Effect> {
    match effect {
        activity::Effect::OpenFilter => {
            model.open_overlay(Overlay::ActivityFilter);
            vec![Effect::None]
        }
        // The dialog's choices are the frame's live ones, so saving is copying them into the file.
        activity::Effect::SaveFilter {
            show_downloads,
            show_builds,
            show_activity_history,
        } => {
            model.settings.config.show_downloads = show_downloads;
            model.settings.config.show_builds = show_builds;
            model.settings.config.show_activity_history = show_activity_history;
            vec![Effect::SaveSettings]
        }
    }
}

fn settings_effect(model: &mut App, effect: settings::Effect) -> Vec<Effect> {
    match effect {
        settings::Effect::SaveSettings => vec![Effect::SaveSettings],
        // A button row is the slice's own command, dispatched exactly as its key would be.
        settings::Effect::Run(command) => dispatch_command(model, command),
    }
}

fn run_effect(model: &mut App, effect: status::Effect) -> Vec<Effect> {
    match effect {
        status::Effect::OpenDetails => {
            model.open_overlay(Overlay::Details);
            vec![Effect::None]
        }
        status::Effect::RequestCancel => {
            model.request_cancel();
            vec![Effect::None]
        }
        status::Effect::ConfirmCancelled => {
            model.confirm_cancel();
            vec![Effect::InterruptActiveJob]
        }
    }
}

fn cleanup_effect(model: &mut App, effect: cleanup::Effect) -> Vec<Effect> {
    match effect {
        cleanup::Effect::Skip => {
            model.skip_cleanup();
            vec![Effect::None]
        }
        cleanup::Effect::Start => {
            model.begin_cleanup_phase();
            vec![Effect::StartCleanup]
        }
        // The same question a finished rebuild asks, opened without a run to settle.
        cleanup::Effect::Ask => {
            model.ask_cleanup();
            vec![Effect::None]
        }
    }
}

fn help_effect(model: &mut App, effect: help::Effect) -> Vec<Effect> {
    match effect {
        help::Effect::Open => {
            model.open_overlay(Overlay::Help);
            vec![Effect::None]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{app, app_with_phase};

    #[test]
    fn quit_and_force_quit_are_distinct_commands_with_distinct_effects() {
        use crate::common::actions::{Action, Effect};

        let mut state = app();
        state.status.phase = Phase::Complete;
        assert_eq!(
            reduce(&mut state, Action::Command(Command::Quit)),
            vec![Effect::Exit(0)],
        );
        assert!(state.should_quit);
        assert!(!state.force_quit, "a normal quit does not kill the child");

        let mut forced = app_with_phase(Phase::Building);
        assert_eq!(
            reduce(&mut forced, Action::Command(Command::ForceQuit)),
            vec![Effect::ForceKillActiveJob, Effect::Exit(130)],
            "a forced quit escalates and reports 130",
        );
        assert!(forced.force_quit);
        assert!(forced.should_quit);
    }
}
