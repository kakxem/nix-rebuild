//! Run state and card tests, driven through the model the frame is composed around.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::dashboard::render_to_string;
use crate::app::model::{App, Overlay};
use crate::app::testing::{FAILED_DRV, app, app_with_phase, mouse, press, run_failed_build};
use crate::common::actions::Command;
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::phase::Phase;
use crate::common::scope::Scope;
use ratatui::layout::Rect;

#[test]
fn the_details_dialog_names_the_run_on_screen() {
    let mut app = app_with_phase(Phase::Complete);
    app.overlay = Overlay::Details;
    app.run_id = Some("20261013-181559-9850".into());

    let screen = render_to_string(&app, 100, 25);
    assert!(screen.contains("run"), "the row is labelled:\n{screen}");
    assert!(
        screen.contains("20261013-181559-9850"),
        "and the id is there to copy while the run is on screen:\n{screen}"
    );
    assert!(
        screen.contains("activities"),
        "the counts fit the box the dialog asks for:\n{screen}"
    );

    // A run that could not be recorded says so instead of showing nothing.
    let mut unrecorded = app_with_phase(Phase::Complete);
    unrecorded.overlay = Overlay::Details;
    assert!(render_to_string(&unrecorded, 100, 25).contains("not recorded"));
}

#[test]
fn d_opens_the_details_of_the_run_on_screen() {
    let mut app = app();
    app.run_id = Some("20261013-181559-9850".into());

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    );
    assert_eq!(
        app.overlay,
        Overlay::Details,
        "the run view has a key for it"
    );

    let documented = crate::modules::panels::status::help_entries()
        .into_iter()
        .any(|entry| {
            entry.key == "D"
                && entry.enter_command()
                    == crate::modules::panels::status::Command::OpenDetails.into()
        });
    assert!(documented, "Help has to name the key that opens Details");
    assert!(
        crate::modules::panels::status::help_entries()
            .into_iter()
            .any(|entry| entry.matches("details") && entry.key == "D"),
        "and the list is searchable by what the row is about",
    );

    press(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn run_card_owns_identity_status_and_ordered_metadata() {
    let mut app = app();
    app.status.phase = Phase::Complete;
    app.status.dirty_tree = true;
    let screen = render_to_string(&app, 110, 25);
    let mut lines = screen.lines();
    let title = lines.next().expect("Run title row");
    let metadata = lines.next().expect("Run metadata row");

    assert!(title.contains("Status"));
    assert!(title.contains("complete"));
    assert!(!screen.contains("nix-rebuild ·"));

    let flake = metadata.find("#sample").expect("flake");
    let scope = metadata.find("Home Manager").expect("scope");
    let action = metadata.find("switch").expect("action");
    let elapsed = metadata.find("00:").expect("elapsed");
    assert!(flake < scope && scope < action && action < elapsed);
    assert!(!metadata.contains("ask"));
}

#[test]
fn the_run_card_names_the_step_of_a_sequence() {
    use crate::common::progress::StepProgress;

    let mut sequence = app();
    sequence.status.step = Some(StepProgress {
        index: 2,
        total: 3,
        label: "home-manager switch".into(),
    });
    let frame = render_to_string(&sequence, 120, 30);
    assert!(frame.contains("2/3 home-manager switch"), "{frame}");

    let single = app();
    let frame = render_to_string(&single, 120, 30);
    assert!(!frame.contains("1/1"), "{frame}");
}

#[test]
fn the_title_carries_the_outcome_and_the_body_carries_the_tree() {
    let mut app = app();
    app.status.dirty_tree = true;
    app.status.phase = Phase::Complete;
    let screen = render_to_string(&app, 110, 25);
    let mut lines = screen.lines();
    let title = lines.next().expect("top border");
    let body = lines.next().expect("Run metadata row");
    assert!(
        !title.contains("dirty"),
        "the title is the outcome and nothing else:\n{screen}"
    );
    assert!(title.contains("complete"));
    assert!(
        body.contains("dirty"),
        "the tree it read belongs beside the flake:\n{screen}"
    );
    assert!(
        body.find("dirty") > body.find("#sample"),
        "and follows the checkout it describes:\n{screen}"
    );
}

#[test]
fn a_narrow_card_drops_the_dirty_word_before_the_flake_name() {
    let mut app = app();
    app.status.dirty_tree = true;
    app.status.phase = Phase::Complete;

    let screen = render_to_string(&app, 60, 12);
    let body = screen.lines().nth(1).expect("Run metadata row");
    assert!(
        body.contains("#sample"),
        "the attribute is what names the configuration:\n{screen}"
    );
    assert!(
        !body.contains("dirty"),
        "the word gives up its cells before the name does:\n{screen}"
    );
    assert!(
        screen.contains("Home Manager"),
        "and the step keeps its column:\n{screen}"
    );
}

#[test]
fn the_run_card_keeps_three_columns_at_every_width() {
    use crate::common::progress::StepProgress;

    let mut app = app();
    app.status.phase = Phase::Complete;
    app.status.flake = "/home/user/.config/nixos-dotfiles#sample".into();
    app.status.step = Some(StepProgress {
        index: 2,
        total: 4,
        label: "nix build home and system".into(),
    });

    for width in [160u16, 120, 100, 80, 60, 44] {
        let screen = render_to_string(&app, width, 12);
        let body = screen.lines().nth(1).expect("Run metadata row");
        assert!(
            body.contains("00:0"),
            "the clock is never the part that goes at {width}:\n{screen}"
        );
        assert!(
            body.contains("Home Manager"),
            "the scope is what the run does, at {width}:\n{screen}"
        );
        assert!(
            screen
                .lines()
                .all(|line| line.chars().count() <= width as usize),
            "no row outgrows the frame at {width}:\n{screen}"
        );
    }
}

#[test]
fn the_step_sits_on_the_cards_centre_whatever_the_flake_costs() {
    let mut app = app();
    app.status.phase = Phase::Complete;

    // The flake is what grows; it must not push the step off the middle of the card.
    for flake in [
        "/repo#sample",
        "/home/user/.config/nixos-dotfiles#sample",
        "/home/user/.config/some-other-checkout-with-a-long-name#other",
    ] {
        app.status.flake = flake.into();
        for width in [150u16, 120, 100, 80] {
            let screen = render_to_string(&app, width, 20);
            let body = screen.lines().nth(1).expect("Run metadata row");
            let cells: Vec<char> = body.chars().collect();
            let start = cells
                .iter()
                .position(|glyph| *glyph == '\u{f04b}')
                .expect("the step's own glyph");
            let end = cells
                .windows(6)
                .position(|word| word == ['s', 'w', 'i', 't', 'c', 'h'])
                .expect("the action")
                + 6;
            let centre = (start + end) / 2;
            assert!(
                centre.abs_diff(width as usize / 2) <= 1,
                "the step is centred on a {width} card whose flake is {flake}, got {centre}:\n{screen}"
            );
        }
    }
}

#[test]
fn the_details_dialog_names_the_tree_it_read() {
    let mut app = app_with_phase(Phase::Complete);
    app.status.dirty_tree = true;
    app.overlay = Overlay::Details;

    let screen = render_to_string(&app, 100, 25);
    assert!(screen.contains("tree"), "the row is labelled:\n{screen}");
    assert!(
        screen.contains("dirty (uncommitted changes)"),
        "a rebuild from an uncommitted tree says so where it can be read:\n{screen}"
    );
}

#[test]
fn run_card_keeps_its_metadata_on_one_compact_row() {
    let mut app = app();
    app.status.phase = Phase::Complete;
    let screen = render_to_string(&app, 100, 25);
    let line = screen
        .lines()
        .find(|line| {
            line.contains('\u{f126}') && line.contains('\u{f04b}') && line.contains('\u{f017}')
        })
        .expect("Run metadata row");
    assert!(line.starts_with('│') && line.ends_with('│'));
}

#[test]
fn the_failure_prompt_names_the_package_and_the_log_command() {
    let mut app = app();
    run_failed_build(&mut app);

    let screen = render_to_string(&app, 120, 30);

    assert!(
        screen.contains("\u{f00d} rebuild failed · nix-rebuild-0.1.0"),
        "the prompt names the package that stopped the run, got:\n{screen}"
    );
    assert!(
        screen.contains("[Enter  Close]"),
        "the way out is offered where the failure is read"
    );
    assert!(
        screen.contains(&format!("nix log {FAILED_DRV}")),
        "the prompt says how to open the full build log"
    );
}

#[test]
fn the_failure_prompt_close_button_ends_the_run() {
    use crate::common::hit_targets::Click;

    let mut state = app();
    run_failed_build(&mut state);
    let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 30);

    let targets = state.click_targets();
    let (close, _) = targets
        .iter()
        .find(|(_, target)| *target == Click::Command(Command::Quit))
        .expect("the prompt's close button");

    mouse(
        &mut state,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.0,
            row: close.1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert!(state.should_quit, "closing the prompt ends the run");
}

#[test]
fn render_failure_prompt_takes_an_optional_button() {
    use crate::common::hit_targets::Click;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut state = app();
    run_failed_build(&mut state);

    let mut clicks = crate::common::hit_targets::ClickMap::default();
    let backend = TestBackend::new(120, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            crate::modules::panels::status::render_failure_prompt(
                frame,
                &state.status,
                Some(("[Enter  Close]", Command::CloseOverlay)),
                None,
                &mut clicks,
                Rect::new(0, 0, 120, 10),
            );
        })
        .expect("render");

    assert!(
        clicks
            .iter()
            .any(|(_, target)| *target == Click::Command(Command::Quit)),
        "the way out is always offered",
    );
    assert!(
        clicks
            .iter()
            .any(|(_, target)| *target == Click::Command(Command::CloseOverlay)),
        "the optional button's own command is recorded",
    );
}

#[test]
fn the_run_card_calls_the_build_step_both_scopes() {
    use crate::common::progress::StepProgress;

    // The scope word and the step label have to agree about what the build step does.
    let mut building = app();
    building.status.scope = Scope::Both;
    building.status.action = "build".into();
    building.status.step = Some(StepProgress {
        index: 2,
        total: 4,
        label: "nix build home and system".into(),
    });

    let frame = render_to_string(&building, 120, 30);
    assert!(frame.contains("Home + System · build"), "{frame}");
    assert!(frame.contains("2/4 nix build home and system"), "{frame}");
}

#[test]
fn late_output_does_not_replace_the_final_completion_status() {
    let mut app = app();
    app.status.phase = Phase::Complete;
    app.status.message = "nothing to do — every path was already in the store".into();
    app.push_output("copying path '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo' from 'https://cache.nixos.org'...");
    assert_eq!(
        app.status.message,
        "nothing to do — every path was already in the store"
    );
}

#[test]
fn cleanup_can_be_cancelled_through_the_same_dialog() {
    let mut app = app();
    app.status.phase = Phase::Cleanup;
    app.request_cancel();
    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    app.confirm_cancel();
    assert!(app.status.cancel_requested);
}

#[test]
fn waiting_for_cleanup_is_a_completed_rebuild_not_an_evaluating_one() {
    let mut state = app();
    state.status.phase = Phase::Evaluating;

    state.request_cleanup_decision();

    assert_eq!(state.status.phase, Phase::Complete);
    assert_eq!(state.overlay, Overlay::CleanupPrompt);
    assert_eq!(
        state.status.message,
        "rebuild complete; cleanup decision required"
    );
}

#[test]
fn elapsed_stops_when_the_rebuild_finishes_and_asks_for_cleanup() {
    let mut app = app();
    app.status.finish_timer();
    let frozen = app.status.elapsed();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        app.status.elapsed(),
        frozen,
        "timer stops at the cleanup prompt"
    );
    app.status.finish_timer();
    assert_eq!(
        app.status.elapsed(),
        frozen,
        "a second call does not restart it"
    );
}

#[test]
fn escape_opens_confirmation_instead_of_immediately_cancelling() {
    let mut app = App::new(
        Scope::Home,
        "/repo#sample".into(),
        false,
        CleanupPolicy::Ask,
    );
    app.status.phase = Phase::Building;

    app.request_cancel();

    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    assert!(!app.status.cancel_requested);

    app.confirm_cancel();
    assert!(app.status.cancel_requested);
}

// Tests live with their slice: new run tests go in this file, not a shared contracts file.

#[test]
fn ctrl_c_closes_a_finished_run_the_way_enter_does() {
    let mut app = app_with_phase(Phase::Complete);
    app.status.exit_code = 0;

    let command = press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );

    assert_eq!(command, Some(Command::Quit));
    assert!(app.should_quit);
    assert_eq!(app.status.exit_code, 0, "the run's own outcome is kept");
}

#[test]
fn ctrl_c_still_asks_before_ending_a_job_that_is_running() {
    let mut app = app_with_phase(Phase::Building);

    let command = press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );

    assert_eq!(
        command,
        Some(crate::modules::panels::status::Command::RequestCancel.into())
    );
    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    assert!(!app.should_quit);
}

#[test]
fn cancellation_dialog_does_not_promise_activation_was_avoided() {
    let mut app = app();
    app.status.phase = Phase::Activating;
    app.request_cancel();
    let screen = render_to_string(&app, 100, 24);
    assert!(screen.contains("Activation may already have changed the system"));
    assert!(!screen.contains("No new generation will be activated"));
}

#[test]
fn cancelling_a_cleanup_names_the_cleanup_not_the_activation() {
    let mut app = app();
    app.status.phase = Phase::Cleanup;
    app.request_cancel();
    let screen = render_to_string(&app, 100, 24);

    assert!(screen.contains("Cancel cleanup?"), "{screen}");
    assert!(
        screen.contains("The rebuild stays activated; nothing else changes."),
        "the question says what is really at stake:\n{screen}"
    );
    assert!(
        !screen.contains("Activation may already have changed the system"),
        "a cleanup cannot touch the activation it followed:\n{screen}"
    );
    assert!(
        screen.contains("[N  Keep cleaning]") && screen.contains("[Y  Cancel cleanup]"),
        "and its answers are about the cleanup:\n{screen}"
    );
}

#[test]
fn ctrl_c_forces_a_quit_from_the_cancel_dialog() {
    let mut app = app();
    app.status.phase = Phase::Building;
    app.request_cancel();
    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    assert!(!app.force_quit);

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );

    assert!(app.force_quit, "Ctrl+C escapes a stuck dialog");
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn ctrl_c_also_forces_a_quit_while_running() {
    let mut app = app();
    app.status.phase = Phase::Building;

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    assert!(!app.force_quit);
}

#[test]
fn escape_still_offers_the_confirmation_dialog() {
    let mut app = app();
    app.status.phase = Phase::Building;
    press(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::ConfirmCancel);
    assert!(!app.force_quit, "Esc is the graceful path");
}

#[test]
fn escape_requests_cancellation_and_enter_confirms_it() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};

    let mut state = app();
    state.status.phase = Phase::Building;

    let emitted = press(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        emitted,
        Some(crate::modules::panels::status::Command::RequestCancel.into())
    );
    assert_eq!(state.overlay, Overlay::ConfirmCancel, "Esc only asks");

    let emitted = press(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert_eq!(
        emitted,
        Some(crate::modules::panels::status::Command::ConfirmCancel.into())
    );
    assert!(state.status.cancel_requested);

    // The confirmation is what asks the runtime to stop the child.
    let mut fresh = app();
    let effects = reduce(
        &mut fresh,
        Action::Command(crate::modules::panels::status::Command::ConfirmCancel.into()),
    );
    assert_eq!(
        effects,
        vec![Effect::InterruptActiveJob],
        "the reducer describes the interruption instead of performing it",
    );
}

#[test]
fn cancellation_choices_are_clickable_buttons() {
    use crate::common::hit_targets::Click;

    let mut state = app();
    state.status.phase = Phase::Activating;
    state.request_cancel();
    let (screen, state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
    assert!(screen.contains("[N  Keep running]"));
    assert!(screen.contains("[Y  Cancel rebuild]"));
    assert!(
        state
            .click_targets()
            .iter()
            .filter(|(_, target)| *target == Click::Command(Command::CloseOverlay))
            .count()
            >= 2,
        "both the close icon and Keep running close the dialog"
    );
    assert!(state.click_targets().iter().any(|(_, target)| *target
        == Click::Command(crate::modules::panels::status::Command::ConfirmCancel.into())));
}

#[test]
fn a_failed_build_names_the_derivation_it_could_not_build() {
    let mut app = app();
    run_failed_build(&mut app);

    let failure = app
        .status
        .first_failure()
        .expect("the error names the derivation");
    assert_eq!(failure.name, "nix-rebuild-0.1.0");
    assert_eq!(failure.drv, FAILED_DRV);

    let row = app
        .activity
        .finished
        .iter()
        .find(|activity| activity.name == "nix-rebuild-0.1.0")
        .expect("the build owns a row");
    assert_eq!(
        row.state,
        crate::modules::panels::activity::ActivityState::Failed,
        "the row that could not build is not reported as done"
    );

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("failed · nix-rebuild-0.1.0"),
        "the Run badge says which package failed, got:\n{screen}"
    );
}

#[test]
fn a_reopened_cleanup_failure_is_not_a_failed_rebuild() {
    use crate::app::model::App;
    use crate::app::testing::run_meta_with_cleanup;

    let meta = run_meta_with_cleanup(
        "probe",
        "home",
        Some(1),
        Some(false),
        "rebuild completed, cleanup failed",
    );

    let app = App::reopen(&meta, &[]);

    assert!(
        !app.rebuild_failed(),
        "the activation succeeded and only the cleanup did not"
    );
    let screen = render_to_string(&app, 120, 30);
    assert!(
        !screen.contains("rebuild failed"),
        "so the card is not given to the banner:\n{screen}"
    );
}

#[test]
fn a_failed_step_names_itself_its_exit_code_and_its_last_error_line() {
    let mut app = app();
    app.status.step = Some(crate::common::progress::StepProgress {
        index: 3,
        total: 3,
        label: "nixos-rebuild switch".into(),
    });
    app.status.note_step_failure(1);
    app.status.note_stderr("sudo: a password is required");
    app.finish_after_exit(true);
    app.status.phase = Phase::Failed;

    let screen = render_to_string(&app, 120, 30);

    assert!(
        screen.contains("rebuild failed · 3/3 nixos-rebuild switch · exit 1"),
        "the step that died and its code are what the run has to name:\n{screen}"
    );
    assert!(
        screen.contains("sudo: a password is required"),
        "and its last word is the reason:\n{screen}"
    );
    assert!(
        !screen.contains("the Logs card has the reason"),
        "a named reason replaces the fallback:\n{screen}"
    );
}

#[test]
fn s_is_the_key_the_status_control_prints() {
    let mut app = app();
    assert!(app.config.show_status, "the card starts on screen");

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    );

    assert!(
        !app.config.show_status,
        "s closes the Status card, the key its footer control prints"
    );
}

#[test]
fn a_reason_line_from_earlier_work_is_not_read_as_the_failure() {
    let mut app = app();
    // Only what the failing step says after it stops is its reason.
    app.status
        .note_stderr("copying path '/nix/store/00000000000000000000000000000000-source'");
    app.status.note_step_failure(1);
    app.finish_after_exit(true);
    app.status.phase = Phase::Failed;

    let screen = render_to_string(&app, 120, 30);

    assert!(
        !screen.contains("copying path"),
        "an earlier step's line is not this failure's reason:\n{screen}"
    );
    assert!(screen.contains("the Logs card has the reason"), "{screen}");
}
