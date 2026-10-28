//! Cleanup tests: policy, prompt answers and engine cancellation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::model::Overlay;
use crate::common::phase::Phase;
use crate::common::scope::Scope;

use crate::app::testing::{app, press};

#[test]
fn cleanup_policy_is_typed_and_labels_itself() {
    use crate::common::cleanup_policy::CleanupPolicy;

    let app = app();
    assert_eq!(app.cleanup.policy, CleanupPolicy::Ask);
    assert_eq!(app.cleanup.policy.label(), "ask");
    assert_eq!(CleanupPolicy::Always.label(), "always");
    assert_eq!(CleanupPolicy::Never.label(), "off");
}

#[test]
fn enter_defaults_to_skipping_cleanup() {
    let mut app = app();
    // The question only appears once a rebuild has finished and is waiting for the answer.
    app.status.phase = Phase::Complete;
    app.overlay = Overlay::CleanupPrompt;
    app.status.message = "rebuild complete; cleanup decision required".into();

    press(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(app.status.phase, Phase::Complete);
    assert_ne!(app.status.phase, Phase::Cleanup);
}

#[test]
fn cleanup_prompt_answers_are_commands() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};

    let mut state = app();
    state.overlay = Overlay::CleanupPrompt;
    let emitted = press(
        &mut state,
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
    );
    assert_eq!(
        emitted,
        Some(crate::modules::work::cleanup::Command::StartCleanup.into())
    );
    assert_eq!(state.overlay, Overlay::None);

    let mut skipped = app();
    skipped.overlay = Overlay::CleanupPrompt;
    let emitted = press(
        &mut skipped,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
    );
    assert_eq!(
        emitted,
        Some(crate::modules::work::cleanup::Command::SkipCleanup.into())
    );

    let mut reducer_only = app();
    reducer_only.overlay = Overlay::CleanupPrompt;
    let effects = reduce(
        &mut reducer_only,
        Action::Command(crate::modules::work::cleanup::Command::SkipCleanup.into()),
    );
    assert_eq!(effects, vec![Effect::None]);
    assert_eq!(reducer_only.overlay, Overlay::None);
}

#[test]
fn cleanup_control_records_cancellation_without_a_child() {
    let control = crate::modules::work::cleanup::CleanupControl::default();
    assert!(!control.is_cancelled());
    control.cancel();
    assert!(control.is_cancelled());
}

#[test]
fn intentional_cleanup_cancellation_is_not_reported_as_an_error() {
    let control = crate::modules::work::cleanup::CleanupControl::default();
    control.cancel();
    let tools = crate::common::tool_paths::ToolPaths {
        nixos_rebuild: "unused".into(),
        home_manager: "unused".into(),
        nix_env: "unused".into(),
        nix_collect_garbage: "unused".into(),
        impure: false,
    };
    let (tx, rx) = crossbeam_channel::unbounded();
    crate::modules::work::cleanup::run(Scope::Home, tools, tx, control, 5);
    let events: Vec<_> = rx.try_iter().collect();
    assert!(!events.iter().any(
        |event| matches!(event, crate::common::terminal::RuntimeEvent::Output(line) if line.starts_with("error:"))
    ));
}

use crate::app::dashboard::render_to_string;
use crate::app::model::App;
use crate::app::testing::mouse;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[test]
fn cleanup_policy_replaces_the_stringly_typed_mode() {
    use crate::common::cleanup_policy::CleanupPolicy;

    let app = App::new(
        Scope::Home,
        "/repo#sample".into(),
        false,
        CleanupPolicy::Never,
    );
    assert_eq!(app.cleanup.policy, CleanupPolicy::Never);

    // The runtime consults the same typed policy the TUI renders.
    let screen = render_to_string(&app, 100, 25);
    assert!(screen.contains("Home Manager"));
}

#[test]
fn the_cleanup_view_lists_every_stage_with_what_it_removes() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.push_output("cleanup: removing generations 651, 650, 649");

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("3 generations (651, 650, 649)"),
        "got:\n{screen}"
    );
    for stage in crate::modules::work::cleanup::CLEANUP_STAGES {
        assert!(screen.contains(stage), "missing stage: {stage}");
    }
}

#[test]
fn a_long_generation_list_counts_the_rest_instead_of_stretching_the_card() {
    let mut app = app();
    app.begin_cleanup_phase();
    let ids: Vec<_> = (0..29).map(|offset| (680 - offset).to_string()).collect();
    app.push_output(&format!("cleanup: removing generations {}", ids.join(", ")));

    let screen = render_to_string(&app, 100, 30);
    assert!(
        screen.contains("29 generations (680, 679, 678, +26 more)"),
        "a long list names three ids and counts the rest:\n{screen}"
    );
    assert!(
        screen.lines().all(|line| line.chars().count() <= 100),
        "no row outgrows the terminal, which is what pushed the block:\n{screen}"
    );
}

#[test]
fn a_single_generation_is_not_called_generations() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.push_output("cleanup: removing generations 651");

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("1 generation (651)"),
        "one removed generation reads as a singular:\n{screen}"
    );
}

#[test]
fn the_cleanup_card_has_no_progress_bar_under_its_stages() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.push_output("cleanup: collecting unreachable store paths");

    let screen = render_to_string(&app, 120, 30);
    assert!(
        !screen.contains(['\u{2591}', '\u{2501}']),
        "the four stages and their own markers carry the progress:\n{screen}"
    );
}

#[test]
fn finished_pending_and_running_stages_carry_different_marks() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.push_output("cleanup: removing generations 651");
    app.push_output("cleanup: collecting unreachable store paths");

    let screen = render_to_string(&app, 120, 30);
    let row = |needle: &str| {
        screen
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_default()
            .to_owned()
    };

    assert!(
        row("Removing old generations").contains('\u{f00c}'),
        "the finished step is ticked"
    );
    let running = row("Finding unreachable store paths");
    assert!(
        !running.contains('\u{f00c}') && !running.contains('\u{f10c}'),
        "the running step is animated, not ticked or empty: {running}"
    );
    assert!(
        row("Optimizing store links").contains('\u{f10c}'),
        "a step still to come is left empty"
    );
}

#[test]
fn every_stage_is_ticked_once_the_cleanup_finishes() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.cleanup.stage = Some(crate::modules::work::cleanup::CLEANUP_STAGES.len());

    let screen = render_to_string(&app, 120, 30);
    assert!(
        !screen.contains('\u{f10c}'),
        "nothing is left pending:\n{screen}"
    );
    for stage in crate::modules::work::cleanup::CLEANUP_STAGES {
        let row = screen
            .lines()
            .find(|line| line.contains(stage))
            .unwrap_or_default();
        assert!(row.contains('\u{f00c}'), "unfinished step: {row}");
    }
}

#[test]
fn the_collector_tally_is_read_and_stays_visible_after_the_cleanup() {
    let mut app = app();
    app.begin_cleanup_phase();
    app.push_output("847 store paths deleted, 3.9 GiB freed");

    assert_eq!(app.cleanup.freed, Some((847, "3.9 GiB".to_owned())));
    let during = render_to_string(&app, 120, 30);
    assert!(
        during.contains("847 paths deleted · 3.9 GiB freed"),
        "the view shows what was freed:\n{during}"
    );

    // The tally must survive on the Run title after the card returns to history.
    app.status.phase = Phase::Complete;
    let after = render_to_string(&app, 120, 30);
    assert!(
        after.contains("847 paths · 3.9 GiB freed"),
        "the tally stays on the Run title:\n{after}"
    );
}

#[test]
fn a_failed_cleanup_says_so_on_the_run_title() {
    let mut app = app();
    app.cleanup.succeeded = Some(false);
    app.status.phase = Phase::Failed;

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("cleanup failed"),
        "the title distinguishes a failed cleanup from a failed rebuild:\n{screen}"
    );
}

#[test]
fn the_cleaning_card_can_show_the_work_instead_of_the_steps() {
    let mut app = app();
    app.activity.start_activity(1, "hello-2.12.3", "download");
    app.finish_activity(1, false);
    app.begin_cleanup_phase();
    app.push_output("cleanup: removing generations 651");

    let steps = render_to_string(&app, 120, 30);
    assert!(steps.contains("Cleaning old generations"));
    assert!(
        !steps.contains("hello-2.12.3"),
        "the steps own the card while they are shown"
    );
    assert!(steps.contains("Show work"), "and offer the way back");

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
    );

    let work = render_to_string(&app, 120, 30);
    assert!(work.contains("hello-2.12.3"), "the work is back");
    assert!(!work.contains("Cleaning old generations"));
    assert!(
        work.contains("Show cleanup"),
        "with a way back to the steps"
    );
}

#[test]
fn cleanup_replaces_stale_activity_history_with_a_live_stage() {
    let mut app = app();
    app.activity.start_activity(1, "old-build", "build");
    app.finish_activity(1, false);
    app.begin_cleanup_phase();
    app.push_output("cleanup: collecting unreachable store paths");

    let screen = render_to_string(&app, 120, 25);
    assert!(screen.contains("Cleaning old generations"));
    assert!(screen.contains("Finding unreachable store paths"));
    assert!(!screen.contains("old-build"));
    assert!(
        !screen.contains("[Filter]"),
        "the table filter is hidden during cleanup"
    );
}

#[test]
fn cleanup_cancellation_is_not_labelled_as_a_cancelled_rebuild() {
    let mut app = app();
    app.status.phase = Phase::Cancelled;
    app.status.message = "cleanup cancelled; rebuild activation remains in place".into();
    let screen = render_to_string(&app, 100, 24);
    assert!(screen.contains("cleanup cancelled"));
    assert!(!screen.contains("rebuild cancelled"));
}

#[test]
fn the_cleanup_prompt_owns_the_frame_until_it_is_answered() {
    let mut app = app();
    app.overlay = Overlay::CleanupPrompt;

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    );
    press(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        app.settings.row, 0,
        "the arrows do not move a dialog behind the question"
    );
    assert_eq!(
        app.overlay,
        Overlay::CleanupPrompt,
        "and nothing opens under it: the question is still waiting"
    );

    // Ctrl+C is the way out of every dialog, so a question can never trap the terminal.
    assert_eq!(
        press(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        Some(crate::common::actions::Command::ForceQuit)
    );
}

#[test]
fn cleanup_answers_name_their_key_once_and_the_footer_never_repeats_them() {
    let mut state = app();
    state.overlay = Overlay::CleanupPrompt;
    let screen = render_to_string(&state, 120, 25);
    assert!(
        screen.contains("[N  No]") && screen.contains("[Y  Yes]"),
        "each answer carries exactly its own key:\n{screen}"
    );
    assert!(
        !screen.contains("Enter/N/Esc"),
        "the keys that also answer (Enter, Esc) are told once, not in both chips:\n{screen}"
    );
    assert!(!screen.contains("Y Clean up"), "the footer stays quiet");
    assert!(!screen.contains("No Skip"), "and offers no second way out");
}

#[test]
fn cleanup_no_and_yes_are_recognizable_mouse_buttons() {
    use crate::common::hit_targets::Click;

    for (command, expected_phase) in [
        (
            crate::modules::work::cleanup::Command::SkipCleanup.into(),
            Phase::Complete,
        ),
        (
            crate::modules::work::cleanup::Command::StartCleanup.into(),
            Phase::Cleanup,
        ),
    ] {
        let mut state = app();
        state.request_cleanup_decision();
        let (screen, mut state) =
            crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
        assert!(screen.contains("[N  No]"));
        assert!(screen.contains("[Y  Yes]"));
        let (area, _) = state
            .click_targets()
            .into_iter()
            .find(|(_, target)| *target == Click::Command(command))
            .unwrap_or_else(|| panic!("{command:?} cleanup button"));

        mouse(
            &mut state,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.0,
                row: area.1,
                modifiers: KeyModifiers::NONE,
            },
            &crate::app::dashboard::Panels::default(),
        );
        assert_eq!(state.status.phase, expected_phase);
        assert_eq!(state.overlay, Overlay::None);
    }
}

#[test]
fn the_question_dialog_breathes_and_its_buttons_sit_where_they_are_clickable() {
    use crate::common::hit_targets::Click;

    let mut state = app();
    state.status.phase = Phase::Complete;
    state.overlay = Overlay::CleanupPrompt;
    let (screen, state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 30);
    let lines: Vec<&str> = screen.lines().collect();

    // Seven rows: top rule, air, question, blank, answers, air, bottom rule.
    let top = lines
        .iter()
        .position(|line| line.contains("[Esc]"))
        .expect("the dialog's top rule carries the corner");
    let question = lines
        .iter()
        .position(|line| line.contains("Clean old generations"))
        .expect("the question");
    let answers = lines
        .iter()
        .position(|line| line.contains("[Y  Yes]"))
        .expect("the answers");
    assert_eq!(
        question,
        top + 2,
        "the air above the body's first row:\n{screen}"
    );
    assert_eq!(
        answers,
        top + 4,
        "one blank row between question and answers"
    );
    assert!(
        lines.get(top + 6).is_some_and(|line| line.contains('╰')),
        "and the air above the bottom rule:\n{screen}"
    );

    let (yes, _) = state
        .click_targets()
        .into_iter()
        .find(|(_, target)| {
            *target == Click::Command(crate::modules::work::cleanup::Command::StartCleanup.into())
        })
        .expect("the yes button");
    assert_eq!(
        usize::from(yes.1),
        answers,
        "the clickable rect is on the row the button is painted on"
    );
    // The chip is drawn from two spans, so a width that drifts would leave half of it dead.
    let chip = "[Y  Yes]";
    let row = lines[answers];
    let picked = row.find(chip).expect("the yes chip on its row");
    let column = row[..picked].chars().count() as u16;
    assert_eq!(
        (yes.0, yes.2),
        (column, chip.chars().count() as u16),
        "the rect covers exactly the cells the chip paints"
    );
}

#[test]
fn a_plain_cleanup_runs_the_stages_and_reports_a_failure() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(format!("plain-cleanup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");

    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
        path.display().to_string()
    };

    let tools = crate::common::tool_paths::ToolPaths {
        nixos_rebuild: "unused".into(),
        home_manager: script("home-manager", "exit 0"),
        nix_env: "unused".into(),
        nix_collect_garbage: script("nix-collect-garbage-ok", "exit 0"),
        impure: false,
    };
    assert!(
        crate::modules::work::cleanup::run_plain(Scope::Home, 5, &tools).is_ok(),
        "an empty generation list is not a failure"
    );

    let failing = crate::common::tool_paths::ToolPaths {
        nix_collect_garbage: script("nix-collect-garbage-fails", "exit 1"),
        ..tools
    };
    assert!(
        crate::modules::work::cleanup::run_plain(Scope::Home, 5, &failing).is_err(),
        "a collector that fails makes the plain cleanup fail"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_control_does_not_poison_the_next_pass() {
    let control = super::CleanupControl::default();
    control.cancel();
    assert!(control.is_cancelled(), "the cancelled flag is latched");

    control.reset();

    assert!(
        !control.is_cancelled(),
        "the next pass starts from a clean control, or every retry dies at once"
    );
}

#[test]
fn the_system_profile_is_pruned_through_the_ticket_the_run_already_holds() {
    let tools = crate::common::tool_paths::ToolPaths::fake();

    assert_eq!(
        crate::modules::work::cleanup::engine::profile_argv(&tools, false),
        vec![
            "sudo".to_owned(),
            "-n".to_owned(),
            "env".to_owned(),
            "NIXOS_REBUILD_PATH=nixos-rebuild".to_owned(),
            "HOME_MANAGER_PATH=home-manager".to_owned(),
            "NIX_ENV_PATH=nix-env".to_owned(),
            "NIX_COLLECT_GARBAGE_PATH=nix-collect-garbage".to_owned(),
            "nix-env".to_owned(),
        ],
        "an update's frame stays unprivileged, so the profile commands ask for the ticket \
         without a prompt and carry the tool path sudo's environment reset would drop"
    );
    assert_eq!(
        crate::modules::work::cleanup::engine::profile_argv(&tools, true),
        vec!["nix-env".to_owned()],
        "the system verb already runs as root"
    );
}

#[test]
fn a_both_pass_prunes_home_then_the_system_profile_through_sudo() {
    use std::os::unix::fs::PermissionsExt;

    // Root's frame needs no ticket, and the argv test above covers that branch.
    if nix::unistd::geteuid().is_root() {
        return;
    }

    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(format!("both-cleanup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");

    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
        path.display().to_string()
    };

    let log = dir.join("sudo.log");
    // A stand-in for sudo: it records what it was asked and runs the rest itself.
    let body = format!(
        "printf '%s\\n' \"$*\" >> {log}\n\
         shift 2\n\
         while [ $# -gt 0 ]; do\n\
           case \"$1\" in\n\
             *=*) shift ;;\n\
             *) break ;;\n\
           esac\n\
         done\n\
         exec \"$@\"",
        log = log.display()
    );
    script("sudo", &body);

    let tools = crate::common::tool_paths::ToolPaths {
        nixos_rebuild: "unused".into(),
        home_manager: script("home-manager", "exit 0"),
        nix_env: script("nix-env", "exit 0"),
        nix_collect_garbage: script("nix-collect-garbage", "exit 0"),
        impure: false,
    };
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    temp_env::with_var("PATH", Some(path), || {
        crate::modules::work::cleanup::run_plain(Scope::Both, 5, &tools)
    })
    .expect("both profiles and the collector ran");

    let calls = std::fs::read_to_string(&log).expect("the sudo stand-in ran");
    assert!(
        calls.contains("--list-generations --profile /nix/var/nix/profiles/system"),
        "the system profile is listed through the ticket: {calls}"
    );
    assert_eq!(
        calls.lines().count(),
        1,
        "the home profile is the user's and needs no ticket: {calls}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
