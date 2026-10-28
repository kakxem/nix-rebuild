//! Tests for the recorded runs: records, listing, and reopening.

use std::time::Duration;

use crate::app::dashboard::render_to_string;

use super::*;

#[test]
fn a_reopened_run_keeps_the_length_and_the_ending_it_had() {
    use super::Meta;
    use crate::app::model::App;

    let started_at = 1_700_000_000;
    let meta = Meta {
        id: "20261008-214733-a3f0".into(),
        scope: "system".into(),
        flake: "/repo#system".into(),
        action: "boot".into(),
        cleanup: "always".into(),
        started_at,
        finished_at: Some(started_at + 754),
        exit_code: Some(0),
        cleanup_ok: Some(false),
        status: "rebuild completed, cleanup failed".into(),
    };

    let app = App::reopen(&meta, &[]);

    assert_eq!(
        app.status.elapsed(),
        Duration::from_secs(754),
        "not a live clock"
    );
    assert_eq!(app.status.phase, Phase::Complete);
    assert_eq!(app.status.scope, Scope::System);
    assert_eq!(app.status.action, "boot");
    assert_eq!(
        app.run_id.as_deref(),
        Some("20261008-214733-a3f0"),
        "a reopened run names the run it is showing",
    );
    assert_eq!(app.cleanup.policy, CleanupPolicy::Always);
    assert_eq!(app.status.message, "rebuild completed, cleanup failed");

    let cancelled = Meta {
        exit_code: Some(130),
        status: "rebuild cancelled; activation state may have changed".into(),
        ..meta
    };
    let app = App::reopen(&cancelled, &[]);
    assert_eq!(app.status.phase, Phase::Cancelled);
    assert_eq!(app.status.exit_code, 130);
}

#[test]
fn a_run_is_recorded_and_reopens_with_its_panes_rebuilt() {
    use super::{Recording, list, listing, load, prune};
    use crate::app::model::App;

    // The one test that owns the state directory, so the others stay pure.
    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(format!("runs-lifecycle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // `temp_env` scopes `XDG_STATE_HOME` to this test and restores it after.
    temp_env::with_var("XDG_STATE_HOME", Some(&dir), || {
        let flake = "/home/user/.config/nixos-dotfiles#sample";
        let mut recording =
            Recording::start(Scope::Home, flake, "switch", CleanupPolicy::Ask).expect("a runs dir");
        recording.record('O', "Activating checkLinkTargets");
        recording.record(
        'J',
        r#"{"action":"start","id":1,"type":100,"fields":["/nix/store/01234567890123456789012345678901-hello-2.12.3"]}"#,
    );
        let id = recording.id().to_owned();

        let mut live = App::new(Scope::Home, flake.into(), false, CleanupPolicy::Ask);
        live.status.exit_code = 0;
        live.status.message = "rebuild and cleanup completed".into();
        recording.finish(
            crate::modules::runs::records::Outcome {
                scope: live.status.scope,
                action: live.status.action.clone(),
                exit_code: live.status.exit_code,
                status: live.status.message.clone(),
            },
            Some(true),
        );

        let listed = list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id, "the newest run is the one recorded");
        assert_eq!(listed[0].flake, flake);
        assert_eq!(listed[0].outcome(), "ok");
        let table = listing();
        assert!(table.contains(&id) && table.contains("CHECKOUT"), "{table}");

        // Reopening rebuilds both panes from the recording, and runs nothing.
        let (meta, records) = load("last").expect("the newest run");
        assert_eq!(records.len(), 2);
        assert!(!records[0].json, "the child's own output");
        assert!(records[1].json, "and the Nix event stream");
        let reopened = App::reopen(&meta, &records);
        assert_eq!(reopened.status.phase, Phase::Complete);
        assert_eq!(reopened.status.exit_code, 0);
        assert_eq!(reopened.status.message, "rebuild and cleanup completed");
        let frame = render_to_string(&reopened, 120, 30);
        assert!(frame.contains("Activating checkLinkTargets"), "{frame}");
        assert!(frame.contains("hello-2.12.3"), "{frame}");

        // Sealing compresses the recording; the same run still reads back.
        let plain = super::runs_dir().join(&id).join("run.log");
        let compressed = super::runs_dir().join(&id).join("run.log.gz");
        assert!(plain.exists(), "a live run keeps its log plain");
        recording.seal();
        assert!(!plain.exists(), "the plain log is gone once sealed");
        let bytes = std::fs::read(&compressed).expect("the compressed log");
        assert_eq!(&bytes[..2], b"\x1f\x8b", "and it is a gzip stream");
        let (_, sealed) = load("last").expect("the sealed run");
        assert_eq!(
            sealed, records,
            "a compressed recording reads back the same records"
        );

        // The oldest runs are dropped, newest kept.
        for old in ["a", "b", "c", "d"] {
            let path = super::runs_dir().join(old);
            std::fs::create_dir_all(&path).expect("dir");
            std::fs::write(
                path.join("meta.conf"),
                format!("id = {old}\nscope = home\n"),
            )
            .expect("meta");
        }
        prune(2);
        let kept: Vec<String> = list().iter().map(|meta| meta.id.clone()).collect();
        assert_eq!(kept, vec!["d".to_string(), "c".to_string()], "newest two");

        // The size ceiling drops the oldest runs, but never the newest one.
        for name in ["e", "f", "g"] {
            let path = super::runs_dir().join(name);
            std::fs::create_dir_all(&path).expect("dir");
            std::fs::write(path.join("run.log"), vec![b'x'; 8]).expect("log");
        }
        super::prune_to(10, 8);
        let names: Vec<String> = super::dirs()
            .into_iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        assert_eq!(
            names,
            vec!["g".to_string()],
            "only the newest run fits, and it is kept even alone"
        );

        // Deleting works by id, and deleting everything reports what went.
        // An id is a directory name, so a path cannot reach outside the records.
        assert!(!super::remove("../runs"), "a path is not a run id");
        assert!(super::load("../runs").is_none());
        assert!(
            super::dirs().iter().any(|dir| dir.ends_with("g")),
            "the records survived the path"
        );
        assert!(super::remove("g"), "a named run is deleted");
        assert!(!super::remove("g"), "deleting it twice reports nothing");
        for name in ["h", "i"] {
            std::fs::create_dir_all(super::runs_dir().join(name)).expect("dir");
        }
        assert_eq!(
            super::remove_all_except(Some("h")),
            1,
            "the card's button keeps the run its own frame is writing"
        );
        assert!(
            super::dirs().iter().any(|dir| dir.ends_with("h")),
            "and the kept run is still there"
        );
        assert_eq!(super::remove_all(), 1, "every remaining run goes");
        assert!(super::dirs().is_empty(), "and no run is recorded");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

use crate::app::testing::{app_with_phase, failed_build_json, failed_build_start_json};
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::phase::Phase;

#[test]
fn a_finished_run_says_its_id_and_how_to_read_it_again() {
    let recorded = |closing: bool| {
        let mut app = app_with_phase(Phase::Complete);
        app.run_id = Some("20261013-215835-201f0".into());
        app.status.message = "rebuild and cleanup completed".into();
        app.closing = closing;
        app
    };

    let screen = render_to_string(&recorded(true), 110, 30);
    assert!(
        screen.contains("Resume this run with:"),
        "the block says what it is for:\n{screen}"
    );
    assert!(
        screen.contains("nix-rebuild runs 20261013-215835-201f0"),
        "and the command that reads it back, ready to copy:\n{screen}"
    );
    assert!(
        !screen.contains("--logs"),
        "the command opens the summary, not the raw log print:\n{screen}"
    );
    assert!(
        !screen.contains("run 20261013-215835-201f0"),
        "the line about the run itself is not missed:\n{screen}"
    );
    assert!(
        !screen.contains("Help]"),
        "the keys that got here give way to it:\n{screen}"
    );

    // A row of air above it, and it costs the cards nothing.
    let air = screen
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .position(|line| line.contains("Resume this run with:"))
        .expect("the closing line");
    assert!(
        screen
            .lines()
            .nth(air - 1)
            .is_some_and(|row| row.trim().is_empty()),
        "the gap above the footer stays empty:\n{screen}"
    );

    // The same run without the block draws the same cards.
    let open = render_to_string(&recorded(false), 110, 30);
    let card_bottom = |rendered: &str| {
        rendered
            .lines()
            .collect::<Vec<_>>()
            .iter()
            .rposition(|line| line.contains('╯'))
            .expect("card bottom")
    };
    assert_eq!(
        card_bottom(&screen),
        card_bottom(&open),
        "the block sits in the room the footer already had"
    );
    assert!(open.contains("Help]"), "the keys are there until the end");

    // An unrecorded run keeps the keys.
    let mut unrecorded = app_with_phase(Phase::Complete);
    unrecorded.closing = true;
    assert!(render_to_string(&unrecorded, 110, 30).contains("Help]"));
}

#[test]
fn a_reopened_failed_run_still_names_the_package_that_broke() {
    use crate::app::model::App;
    use crate::common::records::{Meta, Record};

    let meta = Meta {
        id: "probe".into(),
        scope: "home".into(),
        flake: "/repo#sample".into(),
        action: "switch".into(),
        cleanup: "never".into(),
        started_at: 0,
        finished_at: Some(60),
        exit_code: Some(1),
        cleanup_ok: None,
        status: "home-manager switch exited with 1".into(),
    };
    let records = vec![
        Record {
            json: true,
            line: failed_build_start_json(),
        },
        Record {
            json: true,
            line: r#"{"action":"stop","id":1}"#.into(),
        },
        Record {
            json: true,
            line: failed_build_json(),
        },
    ];

    let app = App::reopen(&meta, &records);

    assert_eq!(app.status.phase, Phase::Failed);
    assert_eq!(
        app.status
            .first_failure()
            .map(|failure| failure.name.as_str()),
        Some("nix-rebuild-0.1.0")
    );
    let row = app
        .activity
        .finished
        .iter()
        .find(|activity| activity.name == "nix-rebuild-0.1.0")
        .expect("the reopened run keeps its rows");
    assert_eq!(
        row.state,
        crate::modules::panels::activity::ActivityState::Failed
    );

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("failed · nix-rebuild-0.1.0"),
        "a reopened failure reads the same way, got:\n{screen}"
    );
}

#[test]
fn leaving_the_frame_wipes_it_and_says_what_the_run_left_behind() {
    use crate::common::config::LeaveScreen;
    use crate::modules::runs::records::{Leave, leave_plan};

    assert_eq!(leave_plan(LeaveScreen::Frame, false), Leave::Frame);
    assert_eq!(leave_plan(LeaveScreen::Frame, true), Leave::Frame);

    // Wiping keeps the closing line only for a run that just happened, not one the reader picked.
    assert_eq!(leave_plan(LeaveScreen::Clean, false), Leave::CleanMessage);
    assert_eq!(leave_plan(LeaveScreen::Clean, true), Leave::CleanSilent);

    // The clean screen carries the frame's own line, colours and all.
    let text = crate::modules::runs::records::closing_text("20261013-221434-258d1");
    let at = |piece: &str| {
        text.find(piece)
            .unwrap_or_else(|| panic!("`{piece}` is in the line: {text:?}"))
    };
    assert!(
        at("Resume this run with: ") < at("nix-rebuild runs "),
        "the label comes first: {text:?}"
    );
    assert!(
        at("nix-rebuild runs ") < at("20261013-221434-258d1"),
        "then the command: {text:?}"
    );
    assert!(
        !text.contains("--logs"),
        "the command opens the summary, not the raw log print: {text:?}"
    );
    assert!(
        text.contains("\x1b[38;2;126;231;207m"),
        "in the frame's own cyan: {text:?}"
    );
    assert!(
        text.contains("\x1b[1;38;2;126;231;207m"),
        "with the id in that cyan and bold: {text:?}"
    );
    assert!(
        text.ends_with("\x1b[0m"),
        "and the style put back at the end: {text:?}"
    );
}

#[test]
fn the_question_before_cutting_the_runs_names_the_count() {
    use crate::app::dashboard::render_to_string_with_state;
    use crate::app::model::Overlay;
    use crate::app::testing::app;

    let mut app = app();
    app.overlay = Overlay::RemoveRunsPrompt;
    app.records.count = 2;
    let (screen, rendered) = render_to_string_with_state(&app, 110, 30);

    assert!(
        screen.contains("Remove all previous runs?"),
        "the question names what it deletes:\n{screen}"
    );
    assert!(
        screen.contains("2 recorded runs will be deleted."),
        "and how many of them:\n{screen}"
    );
    assert!(
        screen.contains("[N  No]") && screen.contains("[Y  Yes]"),
        "with both answers offered:\n{screen}"
    );

    assert_eq!(
        confirm_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE
        )),
        Some(Command::ConfirmRemove.into()),
        "`y` confirms"
    );
    assert_eq!(
        confirm_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE
        )),
        Some(crate::common::actions::Command::CloseOverlay),
        "and Esc declines without touching the runs"
    );

    // The answer that deletes is the only one that names the work.
    let mut answered = rendered;
    let effects = crate::app::reduce::reduce(
        &mut answered,
        crate::common::actions::Action::Command(Command::ConfirmRemove.into()),
    );
    assert_eq!(effects, vec![crate::common::actions::Effect::RemoveRuns]);
}

#[test]
fn the_question_counts_what_is_recorded_when_it_opens() {
    use crate::app::model::Overlay;
    use crate::app::testing::app;

    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(format!("runs-question-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    temp_env::with_var("XDG_STATE_HOME", Some(&dir), || {
        let recorded = dir.join("nix-rebuild/runs");
        let mut empty = app();
        crate::app::runtime::ask_remove_runs(&mut empty);
        assert_eq!(empty.overlay, Overlay::RemoveRunsPrompt);
        assert_eq!(empty.records.count, 0, "nothing recorded asks over zero");

        for name in ["a", "b"] {
            std::fs::create_dir_all(recorded.join(name)).expect("a run");
        }
        let mut recorded = app();
        crate::app::runtime::ask_remove_runs(&mut recorded);
        assert_eq!(recorded.records.count, 2);
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn the_question_keeps_its_own_clicks_over_the_settings_dialog() {
    use crate::app::model::Overlay;
    use crate::app::testing::app;
    use crate::common::hit_targets::Click;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    let mut app = app();
    app.overlay = Overlay::Settings;
    // The dialog's own button asks for the journal to go, so the question lands on top of it.
    app.open_overlay(Overlay::RemoveRunsPrompt);
    app.records.count = 3;
    let (_, rendered) = crate::app::dashboard::render_to_string_with_state(&app, 110, 30);

    let (area, _) = rendered
        .click_targets()
        .into_iter()
        .find(|(_, target)| *target == Click::Command(Command::ConfirmRemove.into()))
        .expect("the answer that deletes");

    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.0,
        row: area.1,
        modifiers: KeyModifiers::NONE,
    };
    // A dialog in front answers from its own click map, whatever the frame behind it laid out.
    let mut probing = rendered.clone();
    let command = crate::app::runtime::handle_mouse_command(
        &mut probing,
        event,
        &crate::app::dashboard::Panels::default(),
    );
    assert_eq!(
        command,
        Some(Command::ConfirmRemove.into()),
        "the button under the pointer answers, not the dialog behind it"
    );
}

#[test]
fn a_run_recorded_before_the_rename_is_listed_read_and_still_pruned() {
    // The state directory is this test's alone, like the lifecycle test's.
    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join(format!("runs-legacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    temp_env::with_var("XDG_STATE_HOME", Some(&dir), || {
        let id = "20251231-235900-leg1";
        let old = dir.join("nix-rebuild/sessions").join(id);
        std::fs::create_dir_all(&old).expect("a legacy run");
        let header = Meta {
            id: id.into(),
            scope: "home".into(),
            flake: "/repo#sample".into(),
            action: "switch".into(),
            cleanup: "ask".into(),
            started_at: 1_767_000_000,
            finished_at: Some(1_767_000_060),
            exit_code: Some(0),
            cleanup_ok: None,
            status: "ok".into(),
        };
        std::fs::write(old.join("meta.conf"), header.text()).expect("its header");
        std::fs::write(old.join("run.log"), "O\tactivating\n").expect("its log");

        assert_eq!(list().len(), 1, "the old directory is still read");
        assert_eq!(super::meta("last").map(|found| found.id), Some(id.into()));
        let (_, records) = load(id).expect("the run opens");
        assert_eq!(records.len(), 1, "and its log comes with it");

        // A new recording lands under runs/, and the list holds both.
        let fresh = Recording::start(Scope::Home, "/repo#sample", "switch", CleanupPolicy::Ask)
            .expect("a runs dir");
        let fresh_id = fresh.id().to_owned();
        fresh.seal();
        assert!(
            runs_dir().join(&fresh_id).is_dir(),
            "new runs are written under runs/"
        );
        assert!(!legacy_dir().join(&fresh_id).exists());
        assert_eq!(list().len(), 2, "old and new in one list");

        prune(1);
        assert_eq!(list().len(), 1, "pruning reaches both directories");
        assert_eq!(list()[0].id, fresh_id, "the newest is the one kept");
    });
}
