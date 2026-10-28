//! The Activity panel's tests.

use crate::app::testing::{FAILED_DRV, app, press};

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::model::{App, Overlay};
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::scope::Scope;

#[test]
fn generic_nix_source_downloads_are_labelled_as_source_trees() {
    let mut app = app();
    app.activity.start_activity(1, "source", "download");

    assert_eq!(app.activity.active[&1].name, "source tree");
    assert_ne!(app.activity.active[&1].name, "source");
}

#[test]
fn cache_probes_do_not_own_activity_rows() {
    let mut app = app();
    // Real shape: one flake input probed per configured cache, one `querying info about …` each.
    for (index, cache) in [
        "https://cache.numtide.com",
        "https://cache.nixos.org",
        "https://vicinae.cachix.org",
    ]
    .iter()
    .enumerate()
    {
        app.ingest_json(&format!(
            r#"{{"action":"start","fields":["/nix/store/01234567890123456789012345678901-source","{cache}"],"id":{index},"level":3,"parent":0,"text":"querying info about '/nix/store/01234567890123456789012345678901-source' on '{cache}'","type":109}}"#
        ));
        app.ingest_json(&format!(r#"{{"action":"stop","id":{index}}}"#));
    }

    assert_eq!(
        app.activity.active.len(),
        0,
        "a metadata probe is not a transfer"
    );
    assert_eq!(
        app.activity
            .visible_activities(app.config.show_builds, app.config.show_downloads)
            .count(),
        0
    );
}

#[test]
fn one_transfer_owns_one_row_even_when_its_path_is_announced_repeatedly() {
    let mut app = app();
    let path = "/nix/store/01234567890123456789012345678901-source";

    // The substitute that brings the path in.
    app.ingest_json(&format!(
        r#"{{"action":"start","fields":["{path}","https://cache.nixos.org"],"id":1,"level":3,"parent":0,"text":"","type":108}}"#
    ));
    app.ingest_json(r#"{"action":"result","fields":[4096,0,0,0],"id":1,"type":105}"#);
    // The copy, then a second announcement of the same path.
    app.ingest_json(&format!(
        r#"{{"action":"start","fields":["{path}","https://cache.nixos.org","local"],"id":2,"level":3,"parent":1,"text":"copying path '{path}' from 'https://cache.nixos.org'","type":100}}"#
    ));
    app.ingest_json(&format!(
        r#"{{"action":"start","fields":["{path}","https://cache.nixos.org"],"id":3,"level":3,"parent":0,"text":"","type":108}}"#
    ));

    assert_eq!(app.activity.active.len(), 1, "one path, one row");
    assert_eq!(app.activity.active[&1].units, 4096);
    assert_eq!(app.activity.active[&1].name, "source tree");
}

#[test]
fn a_path_fetched_again_after_finishing_gets_a_fresh_row() {
    let mut app = app();
    let path = "/nix/store/01234567890123456789012345678901-source";
    let substitute = format!(
        r#"{{"action":"start","fields":["{path}","https://cache.nixos.org"],"id":1,"level":3,"parent":0,"text":"","type":108}}"#
    );
    app.ingest_json(&substitute);
    app.ingest_json(r#"{"action":"stop","id":1}"#);

    app.ingest_json(&substitute.replace(r#""id":1"#, r#""id":2"#));

    assert_eq!(
        app.activity.active.len(),
        1,
        "the second fetch is live and owns its own row"
    );
    assert_eq!(app.activity.active[&2].name, "source tree");
}

#[test]
fn the_reported_size_never_shrinks_the_bar() {
    let mut app = app();
    app.activity.start_activity(1, "archive", "download");
    app.ingest_json(r#"{"action":"result","fields":[100,5000,0,0],"id":1,"type":105}"#);
    app.ingest_json(r#"{"action":"result","fields":[200,0,0,0],"id":1,"type":105}"#);

    assert_eq!(
        app.activity.active[&1].total, 5000,
        "a later zero must not erase a known size"
    );
}

#[test]
fn a_flake_input_fetch_owns_a_row_named_after_its_url() {
    let mut app = app();
    // What `nix flake update` writes: a URL-addressed transfer, then its byte counts.
    app.ingest_json(
        r#"{"action":"start","fields":["https://api.github.com/repos/numtide/flake-utils/commits/HEAD"],"id":5,"level":4,"parent":0,"text":"downloading 'https://api.github.com/repos/numtide/flake-utils/commits/HEAD'","type":101}"#,
    );
    app.ingest_json(r#"{"action":"result","fields":[0,0,0,0],"id":5,"type":105}"#);
    app.ingest_json(r#"{"action":"result","fields":[4318,5756,0,0],"id":5,"type":105}"#);

    let activity = app.activity.active.get(&5).expect("the fetch owns a row");
    assert_eq!(activity.name, "api.github.com/…/commits/HEAD");
    assert_eq!(
        activity.kind,
        crate::modules::panels::activity::ActivityKind::Download
    );
    assert_eq!(activity.units, 4318);
    assert_eq!(activity.total, 5756);
    assert_eq!(
        activity.state,
        crate::modules::panels::activity::ActivityState::Running,
        "a transfer that moves bytes is live"
    );
}

#[test]
fn one_url_fetch_owns_one_row_when_nix_announces_it_again() {
    let mut app = app();
    for id in [5, 6] {
        app.ingest_json(&format!(
            r#"{{"action":"start","fields":["https://channels.nixos.org/flake-registry.json"],"id":{id},"level":4,"parent":0,"text":"","type":101}}"#
        ));
    }

    assert_eq!(app.activity.active.len(), 1, "one URL, one row");
    assert_eq!(
        app.activity.active[&5].name,
        "channels.nixos.org/flake-registry.json"
    );
}

#[test]
fn build_log_phase_is_used_when_nix_emits_no_phase_event() {
    let mut app = app();
    app.activity.start_activity(9, "hello-2.12", "build");
    app.ingest_json(
        r#"{"action":"result","id":9,"type":101,"fields":["Running phase: installPhase"]}"#,
    );
    assert_eq!(app.activity.active[&9].phase, "installing");
}

#[test]
fn activity_scroll_clamps_to_the_visible_rows() {
    let mut app = app();
    for i in 0..100 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    app.activity
        .set_activity_viewport(5, app.config.show_builds, app.config.show_downloads);
    app.activity
        .scroll_activities_by(10_000, app.config.show_builds, app.config.show_downloads);
    assert_eq!(app.activity.scroll, 95);
}

#[test]
fn mouse_wheel_scrolls_the_activity_list() {
    let mut app = app();
    for i in 0..30 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    assert_eq!(app.activity.scroll, 0);
    app.activity
        .scroll_activities_by(4, app.config.show_builds, app.config.show_downloads);
    assert_eq!(app.activity.scroll, 4);
    app.activity
        .scroll_activities_by(-2, app.config.show_builds, app.config.show_downloads);
    assert_eq!(app.activity.scroll, 2);
}

#[test]
fn activity_scroll_clamps_to_the_number_of_rows() {
    let mut app = app();
    for i in 0..5 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    app.activity
        .scroll_activities_by(100, app.config.show_builds, app.config.show_downloads);
    assert!(
        app.activity.scroll <= 5,
        "scrolling stops at the oldest row, got {}",
        app.activity.scroll,
    );
}

#[test]
fn activity_order_is_finished_then_running_then_queued() {
    let mut app = app();
    app.activity
        .start_activity(1, "queued-download", "download");
    app.activity.start_activity(2, "finished-build", "build");
    app.finish_activity(2, false);
    app.activity.start_activity(3, "running-build", "build");

    let names: Vec<_> = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .map(|activity| activity.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["finished-build", "running-build", "queued-download"]
    );
}

#[test]
fn nested_fetch_progress_updates_the_outer_download_without_duplicate_rows() {
    let mut app = app();
    app.ingest_json(
        r#"{"action":"start","id":7,"type":100,"fields":["/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo-1.0","https://cache.nixos.org"]}"#,
    );
    app.ingest_json(
        r#"{"action":"start","id":8,"parent":7,"type":101,"fields":["https://cache.nixos.org/nar/abc.nar.xz"]}"#,
    );
    app.ingest_json(r#"{"action":"result","id":8,"type":105,"fields":[8192]}"#);

    assert_eq!(app.activity.active.len(), 1);
    assert_eq!(app.activity.active[&7].units, 8192);
}

#[test]
fn nix_progress_event_updates_download_bytes() {
    let mut app = App::new(
        Scope::Home,
        "/repo#sample".into(),
        false,
        CleanupPolicy::Ask,
    );
    app.ingest_json(
        r#"{"action":"start","id":7,"type":100,"fields":["/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo-1.0","https://cache.nixos.org"]}"#,
    );
    app.ingest_json(r#"{"action":"result","id":7,"type":105,"fields":[4096]}"#);

    let activity = app.activity.active.get(&7).expect("download activity");
    assert_eq!(activity.units, 4096);
    let active_at = activity.active_at.expect("active transfer timestamp");
    assert_eq!(
        activity.elapsed_at(active_at + Duration::from_secs(1)),
        Duration::from_secs(1)
    );
}

#[test]
fn activity_filter_opens_with_t_and_toggles_downloads() {
    let mut state = app();
    press(
        &mut state,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
    );
    assert_eq!(state.overlay, Overlay::ActivityFilter);

    press(
        &mut state,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(!state.config.show_downloads);
}

use crate::app::dashboard::render_to_string;
use crate::app::testing::{app_with_phase, mouse};
use crate::common::phase::Phase;
use crate::modules::panels::activity::ActivityState;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[test]
fn a_download_that_reports_its_size_shows_the_bytes_it_moved() {
    let mut app = app();
    app.activity.start_activity(1, "archive", "download");
    // Field order: bytes moved, then expected size.
    app.ingest_json(r#"{"action":"result","fields":[1369,10865,0,0],"id":1,"type":105}"#);
    app.ingest_json(r#"{"action":"result","fields":[2747,10865,0,0],"id":1,"type":105}"#);

    let activity = &app.activity.active[&1];
    assert_eq!(activity.units, 2747);
    assert_eq!(activity.total, 10865);

    let screen = render_to_string(&app, 120, 25);
    let row = screen
        .lines()
        .find(|line| line.contains("archive"))
        .expect("download row");
    assert!(
        row.contains("2.7 KiB/10.6 KiB"),
        "the transfer column carries the real bytes, got {row}"
    );
    assert!(
        !row.contains('%'),
        "a percentage is invented for every row that is not a transfer, got {row}"
    );
}

#[test]
fn a_download_with_no_reported_size_shows_only_its_bytes() {
    let mut app = app();
    app.activity.start_activity(1, "archive", "download");
    app.ingest_json(r#"{"action":"result","fields":[5461,0,0,0],"id":1,"type":105}"#);

    assert_eq!(
        app.activity.active[&1].fraction(),
        None,
        "no size, no fraction"
    );

    let screen = render_to_string(&app, 120, 25);
    let row = screen
        .lines()
        .find(|line| line.contains("archive"))
        .expect("download row");
    assert!(
        !row.contains('%'),
        "no percentage without a size, got {row}"
    );
    assert!(
        row.contains("5.3 KiB"),
        "bytes stand in for the percentage, got {row}"
    );
    assert!(
        crate::common::theme::SPINNER_FRAMES
            .iter()
            .any(|frame| row.contains(frame)),
        "a live row keeps moving even without a fraction, got {row}"
    );
}

#[test]
fn the_table_keeps_two_columns_of_air_on_each_side() {
    let mut app = app();
    app.activity
        .start_activity(1, "finished-package", "download");
    app.finish_activity(1, false);
    let screen = render_to_string(&app, 100, 25);
    let header = screen
        .lines()
        .find(|line| line.contains("package"))
        .expect("the table header");
    // The border, two cells of air, then the empty glyph column before the header's word: a
    // Nerd Font glyph draws wider than its cell, so one column reads as no gap at all.
    assert!(
        header.starts_with("\u{2502}     package"),
        "the air before the table: {header}"
    );
    let chars: Vec<char> = header.chars().collect();
    let trailing = chars[..chars.len() - 1]
        .iter()
        .rev()
        .take_while(|cell| **cell == ' ')
        .count();
    assert!(
        trailing >= 2,
        "two columns of air after the last one, got {trailing}: {header}"
    );
    let row = screen
        .lines()
        .find(|line| line.contains("finished-package"))
        .expect("the finished row");
    assert!(
        row.starts_with("\u{2502}   "),
        "the glyph sits two cells inside the border: {row}"
    );
}

#[test]
fn a_completed_row_is_a_green_mark_not_a_filled_bar() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    let mut app = app();
    app.activity
        .start_activity(1, "finished-package", "download");
    app.finish_activity(1, false);
    let mut terminal = Terminal::new(TestBackend::new(120, 25)).expect("test terminal");
    terminal
        .draw(|frame| {
            crate::app::dashboard::render(frame, &mut app);
        })
        .expect("render activity");

    let screen = render_to_string(&app, 120, 25);
    let line_index = screen
        .lines()
        .position(|line| line.contains("finished-package"))
        .expect("the finished row is on screen");
    let width = terminal.backend().buffer().area.width as usize;
    let cells = terminal.backend().buffer().content()[line_index * width..(line_index + 1) * width]
        .to_vec();

    assert!(
        !cells.iter().any(|cell| cell.symbol() == "█"),
        "a finished row is a check, not a filled slab"
    );
    let green = Color::Rgb(166, 218, 75);
    assert!(
        cells.iter().any(|cell| cell.style().fg == Some(green)),
        "its outcome keeps the green mark"
    );
}

#[test]
fn transferred_bytes_own_the_transfer_column() {
    let mut app = app();
    app.activity.start_activity(1, "archive", "download");
    let activity = app.activity.active.get_mut(&1).expect("download activity");
    activity.state = crate::modules::panels::activity::ActivityState::Running;
    activity.units = 204 * 1024 * 1024;

    let screen = render_to_string(&app, 120, 25);
    let header = screen
        .lines()
        .find(|line| line.contains("transfer"))
        .expect("activity header");
    assert!(
        header.contains("time"),
        "the clock column stays, got {header}"
    );
    let row = screen
        .lines()
        .find(|line| line.contains("archive"))
        .expect("download row");
    assert!(row.contains("204.0 MiB"), "got {row}");
    assert!(
        !row.contains('━') && !row.contains('█'),
        "no bar without a known total, got {row}"
    );
}

#[test]
fn running_rows_carry_a_spinner_beside_their_phase() {
    let mut app = app();
    app.ingest_json(
        r#"{"action":"start","fields":["/nix/store/01234567890123456789012345678901-hermes-tui-0.0.1.drv"],"id":4,"level":3,"parent":0,"text":"","type":105}"#,
    );
    app.ingest_json(r#"{"action":"result","fields":["configurePhase"],"id":4,"type":104}"#);

    let screen = render_to_string(&app, 120, 25);
    let row = screen
        .lines()
        .find(|line| line.contains("hermes-tui-0.0.1"))
        .expect("build row");
    assert!(row.contains("configuring"), "the phase is named, got {row}");
    assert!(
        crate::common::theme::SPINNER_FRAMES
            .iter()
            .any(|frame| row.contains(frame)),
        "a running row keeps moving, got {row}"
    );
}

#[test]
fn a_build_never_shows_a_percentage_or_a_filled_bar() {
    let mut app = app();
    app.ingest_json(
        r#"{"action":"start","fields":["/nix/store/01234567890123456789012345678901-hermes-agent-0.0.0.drv"],"id":4,"level":3,"parent":0,"text":"","type":105}"#,
    );
    app.finish_activity(4, false);

    let screen = render_to_string(&app, 120, 25);
    let header = screen
        .lines()
        .find(|line| line.contains("transfer"))
        .expect("activity header");
    assert!(
        !header.contains("progress") && !header.contains('%'),
        "the invented percentage column is gone, got {header}"
    );
    let row = screen
        .lines()
        .find(|line| line.contains("hermes-agent-0.0.0"))
        .expect("build row");
    assert!(row.contains("done"), "got {row}");
    assert!(
        !row.contains('%') && !row.contains('█') && !row.contains('━'),
        "a finished build is a check, not a filled slab, got {row}"
    );
}

#[test]
fn a_running_rebuild_offers_the_cancel_chip_and_keeps_the_filter_while_cleaning() {
    use crate::app::testing::{app_with_phase, mouse};
    use crate::common::hit_targets::Click;
    use crate::common::phase::Phase;
    use crate::modules::panels::status::Command as RunCommand;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    // Running: the card carries the way out, wired to the command the key sends.
    let state = app_with_phase(Phase::Evaluating);
    let (screen, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
    assert!(
        screen.contains("[Esc]"),
        "the card offers the cancel:\\n{screen}"
    );
    let (area, _) = state
        .click_targets()
        .into_iter()
        .find(|(_, target)| *target == Click::Command(RunCommand::RequestCancel.into()))
        .expect("the cancel chip is clickable");
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
    assert_eq!(
        state.overlay,
        Overlay::ConfirmCancel,
        "a click asks before it stops the process group"
    );

    // Terminal: nothing is left to cancel, so the chip is not offered.
    let done = app_with_phase(Phase::Complete);
    assert!(
        !render_to_string(&done, 120, 25).contains("[Esc]"),
        "a finished run has nothing to cancel"
    );

    // Cleaning, on the history view: the filter still applies to the rows being listed.
    let mut cleaning = app_with_phase(Phase::Cleanup);
    cleaning.config.show_cleanup_progress = false;
    let screen = render_to_string(&cleaning, 120, 25);
    assert!(
        screen.contains("Filter") && screen.contains("Show cleanup"),
        "the chips follow the view drawn, not the phase:\\n{screen}"
    );
}

#[test]
fn an_empty_activity_card_animates_while_the_first_step_runs() {
    let app = app_with_phase(Phase::Evaluating);

    let screen = render_to_string(&app, 100, 25);
    assert!(
        screen.contains("evaluating the flake"),
        "the card still names the phase, got {screen}"
    );
    assert!(
        crate::common::theme::SPINNER_FRAMES
            .iter()
            .any(|frame| screen.contains(frame)),
        "an empty card still shows that work is happening, got {screen}"
    );
}

#[test]
fn scrollbar_appears_only_when_the_activity_list_overflows() {
    let mut app = app();
    for i in 0..40 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    let full = render_to_string(&app, 100, 30);
    assert!(
        full.contains('\u{2503}') || full.contains('\u{2588}'),
        "activity scrollbar drawn",
    );
}

#[test]
fn activity_title_uses_one_filter_instead_of_fetch_and_build_toggles() {
    let app = app();
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Activity") && l.contains("Filter"))
        .expect("activity title row");
    assert!(title.contains("Filter"), "the Activity filter is visible");
    assert!(
        !title.contains("Activity Filter"),
        "the panel title already supplies the Activity context"
    );
    assert!(
        !title.contains("Fetch"),
        "Fetch moved into the filter dialog"
    );
    assert!(
        !title.contains("Build"),
        "Build moved into the filter dialog"
    );
}

#[test]
fn card_titles_have_compact_horizontal_padding() {
    let app = app();
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Activity") && l.contains("Filter"))
        .expect("activity title row");
    // Compare in character cells: the border and rule are multi-byte.
    let chars: Vec<char> = title.chars().collect();
    let label_at = chars
        .iter()
        .position(|c| *c == 'A')
        .expect("the Activity label");
    assert_eq!(
        chars.get(label_at.saturating_sub(2)..label_at),
        Some(&['─', ' '][..]),
        "the card title starts after a short rule"
    );
    let control_end = chars
        .iter()
        .rposition(|c| *c == ']')
        .expect("button end marker");
    assert_eq!(
        chars.get(control_end + 1..control_end + 4),
        Some(&['─', '─', '─'][..]),
        "the Activity control inset continues the border rule",
    );
}

#[test]
fn activity_filter_reports_when_it_is_narrowing_the_list() {
    use crate::common::theme::{GREEN, MUTED};

    // The chip is the only place the card says the list is filtered, so its colour is
    // the signal: lit while narrowing, dim when everything is listed.
    let mut narrowing = app();
    narrowing.config.show_downloads = true;
    narrowing.config.show_builds = false;
    assert_eq!(
        filter_chip_color(&narrowing),
        GREEN,
        "the chip is lit while it narrows the list"
    );

    let everything = app();
    assert_eq!(
        filter_chip_color(&everything),
        MUTED,
        "and dim when every kind is listed"
    );
}

/// The Filter chip's icon colour, read from the cells the dashboard painted.
fn filter_chip_color(app: &App) -> ratatui::style::Color {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(120, 25);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut owned = app.clone();
    terminal
        .draw(|frame| {
            crate::app::dashboard::render(frame, &mut owned);
        })
        .expect("render");

    let buffer = terminal.backend().buffer();
    let text = crate::common::ui::buffer_to_string(buffer);
    let row = text
        .lines()
        .position(|line| line.contains("Activity") && line.contains("Filter"))
        .expect("the Activity title row") as u16;
    let column = text
        .lines()
        .nth(row as usize)
        .expect("the row just found")
        .chars()
        .position(|cell| cell == '\u{f0b0}')
        .expect("the Filter chip icon") as u16;

    buffer.cell((column, row)).expect("the chip cell").fg
}

#[test]
fn d_key_no_longer_toggles_downloads() {
    let mut app = app();
    app.config.show_downloads = true;
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    );
    assert!(app.config.show_downloads, "d is no longer bound to fetch");
}

#[test]
fn activity_scrollbar_track_is_clickable() {
    let mut app = app();
    for i in 0..40 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    let (_, mut app) = crate::app::dashboard::render_to_string_with_state(&app, 110, 25);
    let bar = app.activity.scrollbar.expect("activity scrollbar geometry");
    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: bar.x,
            row: bar.y,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert!(
        app.activity.scroll > 0,
        "clicking the top of the track jumps back"
    );
}

#[test]
fn scrolling_activity_rows_changes_what_is_rendered() {
    let mut app = app();
    for i in 0..30 {
        app.activity.start_activity(i, &format!("pkg-{i}"), "build");
        app.finish_activity(i, false);
    }
    // The render registers the panel's row count; keep the post-render state.
    let (newest, rendered) = crate::app::dashboard::render_to_string_with_state(&app, 100, 30);
    assert!(
        newest.contains("pkg-29"),
        "follow mode shows the newest row"
    );
    assert!(
        rendered.activity.viewport > 0,
        "the viewport was registered"
    );

    // Move back by a full screenful so the newest rows leave the view.
    app.activity.set_activity_viewport(
        rendered.activity.viewport,
        app.config.show_builds,
        app.config.show_downloads,
    );
    app.activity.scroll_activities_by(
        rendered.activity.viewport as i64,
        app.config.show_builds,
        app.config.show_downloads,
    );
    let (scrolled, _) = crate::app::dashboard::render_to_string_with_state(&app, 100, 30);
    assert!(
        !scrolled.contains("pkg-29"),
        "scrolled view no longer shows the newest row",
    );
}

#[test]
fn download_filter_hides_only_download_rows() {
    let mut app = App::new(
        Scope::System,
        "/repo#system".into(),
        false,
        CleanupPolicy::Ask,
    );
    app.activity.start_activity(1, "hello-2.12.3", "download");
    app.activity.start_activity(2, "kernel-7.3", "build");
    app.config.show_downloads = false;

    let screen = render_to_string(&app, 100, 26);

    assert!(!screen.contains("hello-2.12.3"));
    assert!(screen.contains("kernel-7.3"));
}

#[test]
fn active_download_without_known_total_shows_transferred_bytes() {
    let mut app = app();
    app.activity.start_activity(1, "demo", "download");
    app.ingest_json(r#"{"action":"result","id":1,"type":105,"fields":[8192]}"#);
    let screen = render_to_string(&app, 100, 24);
    assert!(screen.contains("8.0 KiB"));
}

#[test]
fn activity_filter_rows_select_on_hover_and_toggle_on_click() {
    let mut state = app();
    state.overlay = Overlay::ActivityFilter;
    let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 110, 30);
    let row = state.activity.filter_rows[1];

    mouse(
        &mut state,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: row.0 + 1,
            row: row.1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert_eq!(state.activity.filter_row, 1, "hover selects the builds row");
    assert!(state.config.show_builds);

    mouse(
        &mut state,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.0 + 1,
            row: row.1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert!(!state.config.show_builds);
}

#[test]
fn work_the_failure_stopped_is_not_blamed_for_it() {
    let mut app = app();
    app.activity.start_activity(1, "linux-kernel", "build");
    app.activity.start_activity(2, "nix-rebuild-0.1.0", "build");

    app.push_output(&format!("error: Cannot build '{FAILED_DRV}'."));
    app.finish_after_exit(true);

    let state = |name: &str| {
        app.activity
            .finished
            .iter()
            .find(|activity| activity.name == name)
            .unwrap_or_else(|| panic!("a row for {name}"))
            .state
    };
    assert_eq!(
        state("nix-rebuild-0.1.0"),
        crate::modules::panels::activity::ActivityState::Failed
    );
    assert_eq!(
        state("linux-kernel"),
        crate::modules::panels::activity::ActivityState::NotBuilt,
        "the row the failure stopped is not a second culprit"
    );

    let screen = render_to_string(&app, 120, 30);
    assert!(screen.contains("not built"), "got:\n{screen}");
}

#[test]
fn a_failed_run_without_a_named_derivation_still_marks_what_it_stopped() {
    let mut app = app();
    app.activity.start_activity(1, "linux-kernel", "build");
    // An evaluation error names no derivation, so in-flight rows are what the run marks.
    app.push_output("error: syntax error in configuration line");
    app.finish_after_exit(true);
    app.status.phase = Phase::Failed;

    let row = app
        .activity
        .finished
        .iter()
        .find(|activity| activity.name == "linux-kernel")
        .expect("row");
    assert_eq!(
        row.state,
        crate::modules::panels::activity::ActivityState::Failed
    );

    let screen = render_to_string(&app, 120, 30);
    assert!(
        screen.contains("\u{f00d} rebuild failed"),
        "the prompt still says how the run ended, got:\n{screen}"
    );
    assert!(
        screen.contains("the Logs card has the reason"),
        "and where to read why"
    );
}

#[test]
fn a_copied_source_owns_one_row_that_counts_its_paths() {
    let mut app = app();
    let copy = |app: &mut App, id: u64, path: &str| {
        app.ingest_json(&format!(
            "{{\"action\":\"start\",\"id\":{id},\"type\":0,\"text\":\"copying '«github:nixos/nixpkgs/e7439b6b14ad3cc35d05608ebca9bce01a25f5f8?narHash=sha256-x»/{path}' to the store\"}}"
        ));
        // The message stops at once; the aggregate row outlives it.
        app.ingest_json(&format!("{{\"action\":\"stop\",\"id\":{id}}}"));
    };
    copy(&mut app, 1, "pkgs/a");
    copy(&mut app, 2, "pkgs/b");
    // Nix repeats the message about a path it has already announced.
    copy(&mut app, 3, "pkgs/a");

    let rows: Vec<&crate::modules::panels::activity::Activity> = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .collect();
    assert_eq!(rows.len(), 1, "one source, one row: {rows:?}");
    assert_eq!(rows[0].name, "nixos/nixpkgs e7439b6b");
    assert_eq!(rows[0].units, 2, "three messages about two distinct paths");

    let screen = render_to_string(&app, 120, 25);
    let row = screen
        .lines()
        .find(|line| line.contains("nixos/nixpkgs e7439b6b"))
        .expect("the source row");
    assert!(
        row.contains("2 paths"),
        "the count carries its unit, got {row}"
    );
    assert!(
        !row.contains("narHash"),
        "the checksum is not part of the name, got {row}"
    );
    assert!(
        row.contains("copying"),
        "a source row says what it is doing, got {row}"
    );
}

#[test]
fn a_cache_probe_folds_into_one_row_per_cache() {
    let mut app = app();
    let probe = |app: &mut App, id: u64, url: &str| {
        app.ingest_json(&format!(
            "{{\"action\":\"start\",\"fields\":[\"{url}\"],\"id\":{id},\"type\":101,\"text\":\"downloading '{url}'\"}}"
        ));
        app.ingest_json(&format!("{{\"action\":\"stop\",\"id\":{id}}}"));
    };
    probe(
        &mut app,
        1,
        "https://cache.nixos.org/1q3fizdchmwha3g2nkj5xm9cx0afrmwz.narinfo",
    );
    probe(
        &mut app,
        2,
        "https://cache.nixos.org/qr04w2q074jwc04z6gnm939ji3y6p3dh.narinfo",
    );
    probe(
        &mut app,
        3,
        "https://cache.numtide.com/1q3fizdchmwha3g2nkj5xm9cx0afrmwz.narinfo",
    );

    let rows: Vec<&crate::modules::panels::activity::Activity> = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .collect();
    assert_eq!(rows.len(), 2, "one row per cache, not per path: {rows:?}");

    let screen = render_to_string(&app, 120, 25);
    assert!(screen.contains("cache.nixos.org"), "{screen}");
    assert!(
        screen.contains("2 paths"),
        "that cache was asked about two paths:\n{screen}"
    );
    assert!(
        !screen.contains("narinfo"),
        "a probe is a question, not a file being fetched:\n{screen}"
    );
}

#[test]
fn the_first_build_closes_the_fetching_rows() {
    let mut app = app();
    app.ingest_json("{\"action\":\"start\",\"id\":1,\"type\":0,\"text\":\"copying '«github:nixos/nixpkgs/e7439b6b14ad3cc35d05608ebca9bce01a25f5f8?narHash=sha256-x»/pkgs/a' to the store\"}");
    app.ingest_json("{\"action\":\"stop\",\"id\":1}");
    app.ingest_json("{\"action\":\"start\",\"fields\":[\"https://cache.nixos.org/1q3fizdchmwha3g2nkj5xm9cx0afrmwz.narinfo\"],\"id\":2,\"type\":101,\"text\":\"downloading\"}");
    app.ingest_json("{\"action\":\"stop\",\"id\":2}");
    app.ingest_json("{\"action\":\"start\",\"fields\":[\"/nix/store/shr8jr02ra79vg1y9k58v87vm81xyqp4-home-manager.drv\",\"\",1,1],\"id\":3,\"type\":105,\"text\":\"building\"}");

    let done = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .filter(|activity| activity.state == ActivityState::Done)
        .count();
    assert_eq!(done, 2, "the source and the cache row are history now");

    // A source announced again after the boundary starts a row of its own.
    app.ingest_json("{\"action\":\"start\",\"id\":4,\"type\":0,\"text\":\"copying '«github:nixos/nixpkgs/e7439b6b14ad3cc35d05608ebca9bce01a25f5f8?narHash=sha256-x»/pkgs/c' to the store\"}");
    let fresh = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .find(|activity| {
            activity.name == "nixos/nixpkgs e7439b6b" && activity.state == ActivityState::Running
        })
        .expect("a fresh row");
    assert_eq!(fresh.units, 1, "and counts from one again");
}

#[test]
fn url_names_stay_readable_in_a_table_cell() {
    use crate::modules::panels::activity::state::url_name;

    assert_eq!(
        url_name("https://channels.nixos.org/flake-registry.json"),
        "channels.nixos.org/flake-registry.json"
    );
    assert_eq!(
        url_name("https://api.github.com/repos/numtide/flake-utils/commits/HEAD"),
        "api.github.com/…/commits/HEAD"
    );
    assert_eq!(
        url_name("https://cache.nixos.org/nar/abc.nar.xz"),
        "cache.nixos.org/nar/abc.nar.xz"
    );
    assert_eq!(url_name("https://example.com"), "example.com");
    assert_eq!(url_name("https://example.com/"), "example.com");
}

#[test]
fn a_real_transfer_is_still_a_download_row() {
    let mut app = app();
    app.ingest_json("{\"action\":\"start\",\"fields\":[\"https://cache.nixos.org/1q3fizdchmwha3g2nkj5xm9cx0afrmwz.nar.xz\"],\"id\":1,\"type\":101,\"text\":\"downloading\"}");
    app.ingest_json("{\"action\":\"result\",\"fields\":[2048,4096,0,0],\"id\":1,\"type\":105}");

    let row = app
        .activity
        .visible_activities(app.config.show_builds, app.config.show_downloads)
        .next()
        .expect("a row");
    assert_eq!(
        row.kind,
        crate::modules::panels::activity::ActivityKind::Download
    );
    assert_eq!(row.units, 2048, "the bytes the record reported");

    let screen = render_to_string(&app, 120, 25);
    assert!(
        screen.contains("KiB"),
        "the path coming in keeps its bytes:\n{screen}"
    );
}

#[test]
fn the_filter_dialog_keeps_a_cell_of_air_inside_its_border() {
    let mut app = app();
    app.overlay = Overlay::ActivityFilter;
    let screen = render_to_string(&app, 90, 26);
    let lines: Vec<&str> = screen.lines().collect();

    let top = lines
        .iter()
        .position(|line| line.contains("Filter ─"))
        .expect("the filter's top rule");
    let option = lines
        .iter()
        .position(|line| line.contains("downloads"))
        .expect("its first option");
    assert_eq!(
        option,
        top + 2,
        "the body starts one cell under the rule, from `dialog_body`:\n{screen}"
    );
    let corner = lines[top]
        .chars()
        .position(|cell| cell == '╭')
        .expect("the dialog's left corner");
    let marker = lines[option]
        .chars()
        .position(|cell| cell == '❯')
        .expect("the selected row's marker");
    assert!(
        marker >= corner + 2,
        "and one cell right of the border, so the toggle never touches it:\n{screen}"
    );
    let hint = lines
        .iter()
        .position(|line| line.contains("Space/Enter toggle"))
        .expect("the filter hint");
    assert!(
        lines.get(hint + 1).is_some_and(|line| line.contains('╰')),
        "while the hint hugs the bottom border, ignoring the body's air:\n{screen}"
    );
    let above: Vec<char> = lines
        .get(hint.wrapping_sub(1))
        .expect("the row above the hint")
        .chars()
        .collect();
    let top_row: Vec<char> = lines[top].chars().collect();
    let left = top_row
        .iter()
        .position(|cell| *cell == '╭')
        .unwrap_or_default();
    let right = top_row
        .iter()
        .rposition(|cell| *cell == '╮')
        .unwrap_or(left);
    let inside: String = above[left.saturating_add(1)..right].iter().collect();
    assert!(
        inside.trim().is_empty(),
        "and the body's last row stays blank, so the hint never touches the last option:\n{screen}"
    );
}

#[test]
fn saving_the_activity_filter_hands_over_the_three_choosers() {
    use crate::common::config::Settings;

    let mut state = crate::modules::panels::activity::State::default();
    let mut settings = Settings {
        show_builds: false,
        ..Settings::default()
    };

    let effects = crate::modules::panels::activity::reduce(
        &mut state,
        &mut settings,
        crate::modules::panels::activity::Command::SaveFilter,
    );

    assert_eq!(
        effects,
        vec![crate::modules::panels::activity::Effect::SaveFilter {
            show_downloads: true,
            show_builds: false,
            show_activity_history: false,
        }],
        "the dialog saves the rows it shows"
    );
    assert!(state.saved, "and its button can say so");

    // A choice after the save makes it a new default again, not a saved one.
    crate::modules::panels::activity::reduce(
        &mut state,
        &mut settings,
        crate::modules::panels::activity::Command::ToggleFetch,
    );
    assert!(!state.saved, "a toggle unsaves what the file holds");
}

#[test]
fn the_build_set_counts_its_derivations_beside_the_title() {
    let mut app = app();
    // Nix counts the derivations of the whole set, never the steps inside one of them.
    app.ingest_json(
        r#"{"action":"start","fields":[],"id":9,"level":0,"parent":0,"text":"","type":104}"#,
    );
    app.ingest_json(r#"{"action":"result","fields":[3,12,0,0],"id":9,"type":105}"#);

    assert_eq!(app.activity.build_progress, Some((3, 12)));
    let screen = render_to_string(&app, 100, 25);
    assert!(
        screen.contains("Activity \u{b7} 3/12"),
        "the title counts the set:\n{screen}"
    );
}

/// The package cells a frame shows, in the order they are painted.
fn painted_names(app: &App) -> Vec<String> {
    let screen = render_to_string(app, 100, 30);
    screen
        .lines()
        .filter_map(|line| {
            let rest = line.split("pkg-").nth(1)?;
            Some(rest[..2].to_owned())
        })
        .collect()
}

#[test]
fn a_view_parked_off_the_end_keeps_its_rows_while_more_finish() {
    let mut app = app();
    for index in 0..40u64 {
        app.activity
            .start_activity(100 + index, &format!("pkg-{index:02}"), "build");
        app.activity.close_activity(
            100 + index,
            crate::modules::panels::activity::ActivityState::Done,
        );
    }
    app.activity.set_activity_viewport(10, true, true);
    app.activity.set_activity_scroll(999, true, true);
    let before = painted_names(&app);
    assert!(
        before.len() > 3,
        "the window is the oldest rows: {before:?}"
    );
    assert!(
        before.iter().all(|name| name.as_str() < "10"),
        "the top of the list: {before:?}"
    );

    // Five more derivations finish while the reader sits at the top.
    for index in 40..45u64 {
        app.activity
            .start_activity(100 + index, &format!("pkg-{index:02}"), "build");
        app.activity.close_activity(
            100 + index,
            crate::modules::panels::activity::ActivityState::Done,
        );
    }
    app.activity.set_activity_viewport(10, true, true);

    assert_eq!(
        painted_names(&app),
        before,
        "new rows must not drag a parked view"
    );
}

#[test]
fn a_view_following_the_end_stays_on_the_newest_rows() {
    let mut app = app();
    for index in 0..40u64 {
        app.activity
            .start_activity(100 + index, &format!("pkg-{index:02}"), "build");
        app.activity.close_activity(
            100 + index,
            crate::modules::panels::activity::ActivityState::Done,
        );
    }
    app.activity.set_activity_viewport(10, true, true);

    assert_eq!(app.activity.scroll, 0, "follow mode never parks");
    let painted = painted_names(&app);
    assert_eq!(
        painted.last().map(String::as_str),
        Some("39"),
        "{painted:?}"
    );
}
