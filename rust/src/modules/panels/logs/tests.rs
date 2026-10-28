//! The Logs panel's tests: the records it keeps, where the window sits, what the filter lets through, and what the copy gathers.

use crate::app::testing::{FAILED_DRV, app, press};

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::model::Overlay;
use crate::common::nix_events::LogKind;

#[test]
fn c_copies_every_log_line_from_the_frame() {
    let mut app = app();
    app.push_output("evaluating hello");

    let command = press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    );

    assert_eq!(
        command,
        Some(crate::modules::panels::logs::Command::CopyLogs.into()),
        "the frame answers `c` with the copy the icon performs"
    );
}

#[test]
fn the_named_clipboard_tool_wins_over_the_search() {
    temp_env::with_var(
        "NIX_REBUILD_CLIPBOARD",
        Some("/run/current-system/sw/bin/xclip"),
        || {
            assert_eq!(
                super::clipboard_commands(),
                vec![("/run/current-system/sw/bin/xclip".to_owned(), "x11")],
                "the tool the elevated frame names is the only one tried"
            );
        },
    );

    temp_env::with_var("NIX_REBUILD_CLIPBOARD", None::<&str>, || {
        assert_eq!(
            super::clipboard_commands()[0].0,
            "wl-copy",
            "without a named tool, the search starts with Wayland"
        );
    });
}

#[test]
fn the_plain_log_keeps_the_lines_nix_wrote_and_names_their_kind() {
    let mut app = app();
    app.push_output("evaluating derivation 'github:NixOS/nixpkgs#hello'");
    app.push_output("error: builder for '/nix/store/abc-hello-2.12.drv' failed");
    app.push_output("copying path '/nix/store/xyz-hello-2.12' from 'https://cache.nixos.org'");

    let text = crate::modules::panels::logs::logs_text(&app.logs);
    assert_eq!(text.lines().count(), 3, "one line per record:\n{text}");
    // The whole line survives, which is what a pipe can grep for; the pane's summary would have shortened these.
    assert!(
        text.contains("error    builder for '/nix/store/abc-hello-2.12.drv' failed"),
        "the kind is a column and the severity prefix is not repeated:\n{text}"
    );
    assert!(
        text.contains("eval     evaluating derivation 'github:NixOS/nixpkgs#hello'"),
        "the kind comes from the same mapping the pane draws:\n{text}"
    );
    assert!(
        text.contains(
            "fetch    copying path '/nix/store/xyz-hello-2.12' from 'https://cache.nixos.org'"
        ),
        "a copied path keeps its store prefix:\n{text}"
    );
}

#[test]
fn page_up_moves_one_log_viewport_and_stays_in_bounds() {
    let mut app = app();
    for i in 0..30 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(7);

    press(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(app.logs.scroll, 7, "PageUp moves a whole viewport");

    for _ in 0..100 {
        press(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    }
    assert_eq!(app.logs.scroll, 23, "and clamps at the oldest record");
}

#[test]
fn page_down_moves_one_log_viewport_back_towards_the_newest() {
    let mut app = app();
    for i in 0..30 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(7);
    app.logs.scroll_logs_by(23);
    assert_eq!(app.logs.scroll, 23);

    press(
        &mut app,
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
    );
    assert_eq!(app.logs.scroll, 16, "PageDown moves a whole viewport back");

    for _ in 0..100 {
        press(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        );
    }
    assert_eq!(app.logs.scroll, 0, "and stops at the newest record");
}

#[test]
fn single_line_keys_move_one_row_and_never_overscroll() {
    let mut app = app();
    for i in 0..30 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(7);

    press(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.logs.scroll, 1);
    press(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.logs.scroll, 0);
}

#[test]
fn end_returns_the_log_view_to_follow_mode() {
    use crate::modules::panels::logs::LogCursor;

    let mut app = app();
    for i in 0..30 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(7);
    app.logs.scroll_logs_by(10);
    assert!(app.logs.cursor.is_anchored());

    // A line arriving while the view is pinned must not move it, only count.
    app.push_output("line 30");
    assert_eq!(app.logs.pending_log_count(), 1);

    press(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));

    assert_eq!(app.logs.cursor, LogCursor::Follow);
    assert_eq!(
        app.logs.pending_log_count(),
        0,
        "the badge clears at the end"
    );
    assert_eq!(app.logs.scroll, 0);
}

#[test]
fn anchored_log_cursor_keeps_the_same_top_sequence_after_append() {
    use crate::modules::panels::logs::LogCursor;

    let mut app = app();
    for i in 0..20 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(5);
    app.logs.scroll_logs_by(5);
    let before = app.logs.visible_log_sequence_window();

    app.push_output("line 20");

    assert_eq!(app.logs.cursor, LogCursor::Anchored);
    assert_eq!(
        app.logs.visible_log_sequence_window(),
        before,
        "incoming lines do not move the anchored window",
    );
    assert_eq!(app.logs.pending_log_count(), 1);
}

#[test]
fn log_history_and_clipboard_keep_entries_beyond_the_old_limit() {
    let mut app = app();
    for i in 0..750 {
        app.push_output(&format!("history line {i}"));
    }

    assert_eq!(app.logs.entries.len(), 750);
    let copied = app.logs.logs_for_clipboard();
    assert!(
        copied
            .lines()
            .next()
            .is_some_and(|line| line == "history line 0"),
        "the oldest record is still the first copied line",
    );
    assert!(copied.contains("history line 749"));
}

#[test]
fn adjacent_duplicate_info_lines_are_kept_once() {
    let mut app = app();
    app.push_output("querying info about missing paths");
    app.push_output("querying info about missing paths");
    assert_eq!(app.logs.entries.len(), 1);
}

#[test]
fn a_line_repeated_a_few_entries_later_is_still_the_same_output() {
    let mut app = app();
    // The shape one input's copies produce: the same path copied again with other copies in between.
    app.push_output("copying '/nix/store/01234567890123456789012345678901-source' to the store");
    app.push_output("copying '/nix/store/01234567890123456789012345678902-source' to the store");
    app.push_output("copying '/nix/store/01234567890123456789012345678901-source' to the store");

    assert_eq!(
        app.logs.entries.len(),
        2,
        "the repeated copy is the same output, not a new event"
    );
}

#[test]
fn the_same_text_at_another_level_is_not_treated_as_a_repeat() {
    let mut app = app();
    app.push_output("warning: 'install' is a deprecated alias for 'add'");
    app.push_output("error: 'install' is a deprecated alias for 'add'");

    assert_eq!(
        app.logs.entries.len(),
        2,
        "a warning and an error are not the same line"
    );
}

#[test]
fn duplicate_error_from_json_and_stderr_is_kept_once() {
    let mut app = app();
    app.push_output("error: evaluation failed");
    app.ingest_json(r#"{"action":"msg","level":0,"msg":"error: evaluation failed"}"#);
    assert_eq!(app.logs.entries.len(), 1);
}

#[test]
fn copies_of_one_inputs_parts_stay_distinguishable() {
    let mut app = app();
    let root = "«github:NousResearch/hermes-agent/bc246e1f8ac8f9de1b8c4c4bf4daf1f6ef50cab1?narHash=sha256-GhuYyNPa%2BQXRg6yG4X%2B0QZ8hq%2BIes8Krl2uIAqhw68k%3D»";
    // The six copies Nix printed for this one input.
    for sub_path in [
        "/",
        "/locales",
        "/optional-mcps",
        "/optional-skills",
        "/plugins",
        "/skills",
    ] {
        app.push_output(&format!("copying '{root}{sub_path}' to the store"));
    }

    let labels: Vec<String> = app
        .logs
        .visible_logs()
        .map(|entry| entry.event.summary().to_owned())
        .collect();
    let mut unique = labels.clone();
    unique.sort();
    unique.dedup();

    assert_eq!(labels.len(), 6);
    assert_eq!(
        unique.len(),
        6,
        "each copied part reads as its own line, got {labels:?}"
    );
    assert_eq!(
        labels[1],
        "copying  github:NousResearch/hermes-agent/locales → store"
    );
    assert_eq!(
        labels[5],
        "copying  github:NousResearch/hermes-agent/skills → store"
    );
}

#[test]
fn the_log_filter_can_isolate_the_degraded_session_warning() {
    let mut app = app();
    app.push_output("Activating reloadSystemd");
    app.push_output("The service manager is degraded.");

    assert_eq!(app.logs.visible_logs().count(), 2);

    // Keep warnings only, so the filter pulls the failed service out of the activation chatter.
    for kind in crate::modules::panels::logs::FILTERABLE_KINDS {
        if kind != LogKind::Warning {
            app.logs.filter.kinds.toggle(kind);
        }
    }
    let visible: Vec<_> = app
        .logs
        .visible_logs()
        .map(|entry| entry.event.raw().to_owned())
        .collect();
    assert_eq!(visible, ["The service manager is degraded."]);
}

#[test]
fn warnings_are_retained_and_hidden_dynamically() {
    let mut app = app();
    app.push_output("warning: option `system` has been renamed");
    assert_eq!(app.logs.visible_logs().count(), 1);

    app.logs.filter.kinds.toggle(LogKind::Warning);
    assert_eq!(app.logs.visible_logs().count(), 0);
    assert_eq!(
        app.logs.entries.len(),
        1,
        "filtering must not discard raw history"
    );
}

#[test]
fn log_scroll_cannot_go_past_the_oldest_line() {
    let mut app = app();
    for i in 0..5 {
        app.push_output(&format!("line {i}"));
    }
    // A panel showing every line has nothing to scroll back to.
    app.logs.set_log_viewport(20);
    app.logs.scroll_logs_by(10_000);
    assert_eq!(
        app.logs.scroll, 0,
        "with every line already visible there is nothing to scroll back to",
    );
}

#[test]
fn log_scroll_clamps_to_the_visible_rows() {
    let mut app = app();
    for i in 0..100 {
        app.push_output(&format!("line {i}"));
    }
    app.logs.set_log_viewport(5);
    app.logs.scroll_logs_by(1_000);
    assert_eq!(
        app.logs.scroll, 95,
        "the oldest line stops at the top of the viewport",
    );
}

#[test]
fn copy_action_produces_the_visible_log_text() {
    let mut app = app();
    app.push_output("first line");
    app.push_output("second line");
    app.logs.set_log_viewport(10);
    let payload = app.logs.logs_for_clipboard();
    assert!(payload.contains("first line"));
    assert!(payload.contains("second line"));
    assert_eq!(
        payload.lines().count(),
        2,
        "every visible line is copied once",
    );
}

#[test]
fn copy_gathers_every_log_not_only_the_visible_window() {
    let mut app = app();
    for i in 0..200 {
        app.push_output(&format!("line {i}"));
    }
    // Only a handful of rows fit on screen.
    app.logs.set_log_viewport(5);
    let payload = app.logs.logs_for_clipboard();
    assert_eq!(
        payload.lines().count(),
        200,
        "the clipboard gets the whole history, not the visible slice",
    );
    assert!(payload.contains("line 0"), "the oldest line is included");
    assert!(payload.contains("line 199"), "the newest line is included");
}

#[test]
fn copy_is_the_raw_log_text_in_order() {
    let mut app = app();
    app.push_output("first");
    app.push_output("second");
    app.push_output("third");
    let payload = app.logs.logs_for_clipboard();
    let lines: Vec<&str> = payload.lines().collect();
    assert_eq!(lines, vec!["first", "second", "third"]);
}

#[test]
fn copy_confirmation_expires_on_its_own() {
    let mut app = app();
    app.logs.arm_copied_marker(std::time::Duration::ZERO);
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(
        app.logs.copied_confirmation_expired(),
        "a zero-length window expires immediately",
    );
}

#[test]
fn f_key_opens_the_log_filter() {
    let mut app = app();
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
    );
    assert_eq!(app.overlay, Overlay::LogFilter);

    app.overlay = Overlay::None;
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE),
    );
    assert_eq!(app.overlay, Overlay::LogFilter);
}

#[test]
fn mouse_wheel_scrolls_the_logs() {
    let mut app = app();
    for i in 0..40 {
        app.push_output(&format!("log line {i}"));
    }
    assert_eq!(app.logs.scroll, 0, "starts in follow mode");

    app.logs.scroll_logs_by(3);
    assert_eq!(app.logs.scroll, 3);

    app.logs.scroll_logs_by(-1);
    assert_eq!(app.logs.scroll, 2);

    app.logs.scroll_logs_by(-40);
    assert_eq!(app.logs.scroll, 0, "cannot scroll past the newest line");
}

#[test]
fn page_commands_move_one_viewport_through_the_reducer() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let mut state = app();
    for i in 0..30 {
        state.push_output(&format!("line {i}"));
    }
    state.logs.set_log_viewport(7);

    reduce(
        &mut state,
        Action::Command(crate::modules::panels::logs::Command::PageLogsUp.into()),
    );
    assert_eq!(state.logs.scroll, 7);

    reduce(
        &mut state,
        Action::Command(crate::modules::panels::logs::Command::PageLogsDown.into()),
    );
    assert_eq!(state.logs.scroll, 0);
}

#[test]
fn copy_is_described_by_the_reducer_not_performed_by_it() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};

    let mut state = app();
    state.push_output("first line");
    state.push_output("second line");

    let effects = reduce(
        &mut state,
        Action::Command(crate::modules::panels::logs::Command::CopyLogs.into()),
    );

    assert_eq!(
        effects,
        vec![Effect::CopyLogs("first line\nsecond line".into())],
        "the payload is the whole history and the reducer only describes it",
    );
    assert!(
        !state.logs.showing_copied(),
        "nothing is confirmed before the clipboard write happens",
    );
}

#[test]
fn tick_retires_an_expired_copy_confirmation() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let mut state = app();
    state.logs.arm_copied_marker(Duration::from_millis(0));
    std::thread::sleep(Duration::from_millis(5));
    reduce(&mut state, Action::Tick);
    assert!(
        !state.logs.showing_copied(),
        "the expired confirmation is retired on the next tick",
    );

    let mut fresh = app();
    fresh.logs.arm_copied_marker(Duration::from_secs(60));
    reduce(
        &mut fresh,
        Action::Command(crate::modules::panels::logs::Command::ToggleRaw.into()),
    );
    assert!(
        fresh.logs.showing_copied(),
        "an unrelated command leaves it alone",
    );
}

#[test]
fn log_filter_opens_with_f_and_toggles_the_selected_kind() {
    use crate::common::nix_events::LogKind;

    let mut state = app();
    press(
        &mut state,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
    );
    assert_eq!(state.overlay, Overlay::LogFilter);

    press(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    press(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    press(
        &mut state,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(!state.logs.filter.kinds.contains(LogKind::Error));
}

#[test]
fn the_same_error_on_both_channels_is_kept_once_even_when_it_is_multi_line() {
    let mut app = app();
    let block = [
        format!("error: Cannot build '{FAILED_DRV}'."),
        "       Reason: builder failed with exit code 101.".to_owned(),
        "       Output paths:".to_owned(),
        "         /nix/store/av2i3dkhf1xzqi71wx3d7a48x8f48dgg-nix-rebuild-0.1.0".to_owned(),
        "       For full logs, run:".to_owned(),
        format!("         nix log {FAILED_DRV}"),
    ];
    // The child's own output, one line at a time.
    for line in &block {
        app.push_output(line);
    }
    assert_eq!(app.logs.entries.len(), block.len());

    // The same failure again, as the JSON event a parent process re-raises.
    let message = block.join("\\n");
    app.ingest_json(&format!(
        r#"{{"action":"msg","column":null,"file":null,"level":0,"line":null,"msg":"{message}"}}"#
    ));

    assert_eq!(
        app.logs.entries.len(),
        block.len(),
        "one failure, one block, whichever channel reported it"
    );
}

// ---- Contract tests ----

use crate::app::dashboard::render_to_string;
use crate::app::model::App;
use crate::app::testing::{click, mouse};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

// ---- One model, one action path -------------------------------------------

/// The toggled view state a command can change, so two dispatch routes can be compared without naming every field.
fn toggles(app: &App) -> (bool, bool, bool, bool) {
    (
        app.config.show_logs,
        app.config.raw_logs,
        app.config.show_downloads,
        app.config.show_builds,
    )
}

#[test]
fn normalized_logs_use_exactly_one_terminal_row_per_record() {
    let mut app = app();
    app.push_output(&format!("first {}", "x".repeat(200)));
    app.push_output(&format!("second {}", "y".repeat(200)));
    app.push_output(&format!("third {}", "z".repeat(200)));

    let screen = render_to_string(&app, 70, 24);
    assert_eq!(screen.matches("first").count(), 1);
    assert_eq!(screen.matches("second").count(), 1);
    assert_eq!(screen.matches("third").count(), 1);

    // Three records on three distinct rows; none wrapped onto a second row.
    let rows: Vec<usize> = ["first", "second", "third"]
        .iter()
        .map(|prefix| {
            screen
                .lines()
                .position(|line| line.contains(prefix))
                .expect("record row")
        })
        .collect();
    assert_eq!(
        rows.len(),
        rows.iter().collect::<std::collections::BTreeSet<_>>().len(),
        "each record is on its own row",
    );
    assert!(
        rows.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "the three records are consecutive rows, got {rows:?}",
    );
}

#[test]
fn long_log_records_are_clipped_not_wrapped_at_every_width() {
    for width in [70u16, 100, 160] {
        let mut app = app();
        app.push_output(&format!("alpha {}", "a".repeat(400)));
        app.push_output(&format!("beta {}", "b".repeat(400)));

        let screen = render_to_string(&app, width, 24);
        assert_eq!(
            screen.matches("alpha").count(),
            1,
            "alpha occupies one row at width {width}",
        );
        assert_eq!(
            screen.matches("beta").count(),
            1,
            "beta occupies one row at width {width}",
        );
    }
}

#[test]
fn raw_mode_is_configured_inside_the_logs_filter() {
    let mut app = app();
    let dashboard = render_to_string(&app, 110, 25);
    let logs_title = dashboard
        .lines()
        .find(|line| line.contains("Logs") && line.contains("Filter"))
        .expect("Logs title row");
    assert!(!logs_title.contains("Raw"));

    app.overlay = Overlay::LogFilter;
    let dialog = render_to_string(&app, 110, 25);
    assert!(dialog.contains("Raw log lines"));
}

#[test]
fn filter_dialog_separates_the_last_option_from_its_keybinds() {
    let mut app = app();
    app.overlay = Overlay::LogFilter;
    let screen = render_to_string(&app, 110, 25);
    let lines: Vec<_> = screen.lines().collect();
    let info = lines
        .iter()
        .position(|line| line.contains("info"))
        .expect("info filter row");
    let hint = lines
        .iter()
        .position(|line| line.contains("Space toggle"))
        .expect("filter keybinds");
    // The button the dialog ends with, a blank body row, then the hint.
    assert_eq!(
        hint,
        info + 4,
        "the save button and a blank body row separate the last option from the hint"
    );
    assert!(
        lines
            .get(info + 2)
            .is_some_and(|line| line.contains("Save as default")),
        "the dialog ends with the button that keeps the filter:\n{screen}"
    );
    assert!(
        lines.get(hint + 1).is_some_and(|line| line.contains('╰')),
        "the hint hugs the bottom border, ignoring the body's air:\n{screen}"
    );
    let above: Vec<char> = lines[hint.wrapping_sub(1)].chars().collect();
    let top = lines
        .iter()
        .position(|line| line.contains("Filter logs ─"))
        .expect("the dialog's top rule");
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
fn raw_filter_row_toggles_with_keyboard_and_mouse() {
    let mut keyboard = app();
    keyboard.overlay = Overlay::LogFilter;
    press(
        &mut keyboard,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    );
    press(
        &mut keyboard,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(keyboard.config.raw_logs);

    let mut pointer = app();
    pointer.overlay = Overlay::LogFilter;
    let (_, mut pointer) = crate::app::dashboard::render_to_string_with_state(&pointer, 110, 25);
    let raw = pointer.logs.filter_rows[1];
    mouse(
        &mut pointer,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: raw.0 + 1,
            row: raw.1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert!(pointer.config.raw_logs);
}

#[test]
fn log_view_has_semantic_labels_instead_of_an_undifferentiated_text_wall() {
    let mut app = app();
    app.push_output("evaluating derivation 'git+file:///repo#home'");
    app.push_output("copying path '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo' from 'https://cache.nixos.org'...");
    app.push_output("building '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo.drv'...");
    app.push_output("Activating Home Manager configuration");
    app.push_output("warning: option `system` has been renamed");

    // Tall enough that all five kinds fit above the padding rows.
    let screen = render_to_string(&app, 120, 34);
    for label in ["eval", "fetch", "build", "activate", "warning"] {
        assert!(
            screen.contains(label),
            "missing semantic log label: {label}"
        );
    }
}

#[test]
fn raw_log_toggle_uses_the_original_line() {
    let mut app = app();
    app.push_output("copying path '/nix/store/01234567890123456789012345678901-hello-2.12' from 'https://cache.nixos.org'...");
    let normalized = render_to_string(&app, 110, 24);
    assert!(normalized.contains("downloading  hello-2.12"));

    app.config.raw_logs = true;
    let raw = render_to_string(&app, 110, 24);
    assert!(raw.contains("copying path '/nix/store/01234567890123456789012345678901-hello-2.12'"));
}

#[test]
fn a_frame_toggle_does_not_touch_the_file() {
    let mut app = app();

    for command in [
        crate::modules::panels::logs::Command::ToggleRaw.into(),
        crate::modules::panels::logs::Command::ToggleLogs.into(),
        crate::modules::panels::status::Command::ToggleStatus.into(),
        crate::modules::panels::activity::Command::ToggleFetch.into(),
        crate::modules::panels::settings::Command::ToggleSettings.into(),
    ] {
        let effects =
            crate::app::reduce::reduce(&mut app, crate::common::actions::Action::Command(command));
        assert!(
            !effects.contains(&crate::common::actions::Effect::SaveSettings),
            "{command:?} is this frame's business, not the file's"
        );
    }
    assert!(app.config.raw_logs);
    assert!(!app.config.show_logs);
    assert!(!app.config.show_status);
    assert!(!app.config.show_downloads);
    assert_eq!(
        app.overlay,
        crate::app::model::Overlay::Settings,
        "the settings key opens the dialog, and nothing about that reaches the file"
    );
    assert!(
        !app.settings.config.raw_logs,
        "and the next start keeps reading what the file said"
    );
}

#[test]
fn scrollbar_appears_only_when_the_logs_overflow() {
    let mut overflowing = app();
    for i in 0..40 {
        overflowing.push_output(&format!("log line {i}"));
    }
    let full = render_to_string(&overflowing, 100, 25);
    assert!(
        full.contains('\u{2503}') || full.contains('\u{2588}'),
        "thumb drawn"
    );
    let fresh = app();
    let empty = render_to_string(&fresh, 100, 25);
    assert!(
        !empty.contains('\u{2503}'),
        "no scrollbar for a panel that fits",
    );
}

#[test]
fn filter_and_copy_drive_the_logs_title_row_without_raw_or_warn_toggles() {
    let mut app = app();
    app.config.show_logs = true;
    app.config.raw_logs = false;
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    assert!(!title.contains("Raw"), "Raw moved into the filter dialog");
    assert!(title.contains("Filter"), "the structured filter is visible");
    assert!(
        !title.contains("Warn"),
        "warnings belong in the filter dialog"
    );
}

#[test]
fn panel_titles_rule_off_the_inset_controls() {
    let mut app = app();
    app.config.show_logs = true;
    app.config.raw_logs = false;
    let screen = render_to_string(&app, 110, 25);

    let logs_title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    let label_at = logs_title.find("Logs").expect("panel label");
    let filter_at = logs_title.find("Filter").expect("filter chip");
    let rule: String = logs_title
        .chars()
        .skip(label_at + 4)
        .take(filter_at - label_at - 4)
        .collect();
    assert!(
        rule.contains('\u{2500}'),
        "a rule separates the panel label from the trailing controls",
    );
}

#[test]
fn panel_content_is_padded_from_the_borders() {
    let mut app = app();
    app.push_output("evaluating derivation 'git+file:///repo#home'");
    let screen = render_to_string(&app, 110, 25);
    let row = screen
        .lines()
        .find(|l| l.contains("eval ") && l.contains('\u{2502}'))
        .expect("log row");
    let border = row.find('\u{2502}').expect("left border");
    let icon = row.find('\u{f002}').expect("eval icon");
    assert!(
        icon > border + 1,
        "content is padded from the left border (border {border}, icon {icon})",
    );
}

#[test]
fn scrollbar_keeps_a_column_of_padding_from_the_border() {
    let mut app = app();
    for i in 0..40 {
        app.push_output(&format!("log line {i}"));
    }
    let screen = render_to_string(&app, 110, 25);
    let track = screen
        .lines()
        .find(|l| l.contains('\u{2503}') || l.contains('\u{2588}'))
        .expect("scrollbar row");
    assert!(
        track.ends_with("\u{2502}") || track.trim_end().ends_with('\u{2502}'),
        "the border still closes the row after the scrollbar",
    );
}

#[test]
fn every_click_target_matches_its_keyboard_key() {
    use crate::common::hit_targets::Click;
    // Each control mirrors the key that is printed on it.
    let cases = [
        (
            crate::modules::panels::logs::Command::ToggleLogs.into(),
            KeyCode::Char('l'),
            KeyModifiers::NONE,
        ),
        (
            crate::modules::panels::logs::Command::ToggleRaw.into(),
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        ),
    ];
    for (command, key, modifiers) in cases {
        let mut by_click = app();
        click(&mut by_click, Click::Command(command));

        let mut by_key = app();
        press(&mut by_key, KeyEvent::new(key, modifiers));

        assert_eq!(
            toggles(&by_click),
            toggles(&by_key),
            "click on {command:?} matches pressing {key:?}",
        );
    }
}

#[test]
fn panel_label_is_padded_from_the_border() {
    let app = app();
    let screen = render_to_string(&app, 110, 25);
    let row = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains('\u{2500}'))
        .expect("logs title row");
    let border = row.find('╭').expect("left card corner");
    let label = row.find("Logs").expect("label");
    assert!(
        label > border + 1,
        "the panel label is indented (border {border}, label {label})",
    );
}

#[test]
fn panel_controls_use_brackets_not_a_middle_dot() {
    let mut app = app();
    app.config.show_logs = true;
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    assert!(!title.contains("Raw"), "Raw is configured in the dialog");
    assert!(title.contains("[\u{f0b0} Filter]"), "Filter is a button");
    assert!(
        !title.contains('\u{b7}'),
        "the middle dot is reserved for status badges, not controls",
    );
}

#[test]
fn logs_offer_a_copy_control() {
    let mut app = app();
    app.push_output("a log line");
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    assert!(
        title.contains('\u{f0c5}') || title.contains('\u{f46d}'),
        "a clipboard glyph is offered in the Logs title",
    );
}

#[test]
fn failure_prompt_offers_a_button_to_copy_the_logs() {
    use crate::app::testing::run_failed_build;
    use crate::common::hit_targets::Click;

    // The failure banner borrows this slice's copy button, so its command must be the Logs panel's own.
    let mut app = app();
    run_failed_build(&mut app);
    let (_, state) = crate::app::dashboard::render_to_string_with_state(&app, 120, 30);

    assert!(
        state.click_targets().iter().any(|(_, target)| *target
            == Click::Command(crate::modules::panels::logs::Command::CopyLogs.into())),
        "the failure prompt carries the Logs panel's copy button",
    );
}

#[test]
fn the_failure_banner_confirms_the_copy_from_its_own_button() {
    use crate::app::reduce::reduce;
    use crate::app::testing::run_failed_build;
    use crate::common::actions::Action;

    // The banner's button is the one clicked while the Logs card is out of sight, so the
    // acknowledgement has to reach that label and not only the panel's glyph.
    let mut banner = app();
    run_failed_build(&mut banner);
    reduce(&mut banner, Action::ClipboardFinished(Ok(())));

    let screen = render_to_string(&banner, 120, 30);
    let buttons = screen
        .lines()
        .find(|line| line.contains("[Enter  Close]"))
        .unwrap_or_else(|| panic!("the banner's button row, got:\n{screen}"));

    assert!(
        buttons.contains("\u{f00c}  Copied"),
        "the banner's copy button acknowledges the write, got:\n{buttons}"
    );

    // The label follows the shared confirmation instead of latching: a window that has
    // already elapsed leaves the plain button where it was.
    let mut expired = app();
    run_failed_build(&mut expired);
    expired.logs.copied_confirm = Duration::ZERO;
    reduce(&mut expired, Action::ClipboardFinished(Ok(())));
    let screen = render_to_string(&expired, 120, 30);
    let buttons = screen
        .lines()
        .find(|line| line.contains("[Enter  Close]"))
        .unwrap_or_else(|| panic!("the banner's button row, got:\n{screen}"));
    assert!(
        buttons.contains("\u{f0c5}  Copy logs"),
        "an expired confirmation is no confirmation, got:\n{buttons}"
    );
}

#[test]
fn panel_title_rule_reaches_both_borders() {
    let app = app();
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    let chars: Vec<char> = title.chars().collect();
    assert_eq!(chars[0], '╭', "the card starts at its left corner");
    assert_eq!(chars[chars.len() - 1], '╮', "and reaches its right corner",);
}

#[test]
fn copy_acknowledges_the_user_then_returns_to_normal() {
    let mut app = app();
    app.push_output("a line");
    let _ = render_to_string(&app, 110, 25);

    // The copy control only asks the loop to write the clipboard; the confirmation arms on the reported success.
    let effects = crate::app::reduce::reduce(
        &mut app,
        crate::common::actions::Action::Command(
            crate::modules::panels::logs::Command::CopyLogs.into(),
        ),
    );
    assert!(
        matches!(
            effects.as_slice(),
            [crate::common::actions::Effect::CopyLogs(_)]
        ),
        "the request is handed to the loop, not performed inline",
    );
    crate::app::reduce::reduce(
        &mut app,
        crate::common::actions::Action::ClipboardFinished(Ok(())),
    );
    assert!(
        app.logs.copied_at.is_some(),
        "the confirmation is armed on success"
    );

    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    assert!(
        title.contains('\u{f00c}'),
        "the clipboard glyph turns into a check mark",
    );

    // After the acknowledge window the control goes back to normal.
    app.logs.clear_copied_marker();
    assert!(app.logs.copied_at.is_none());
    let screen = render_to_string(&app, 110, 25);
    let title = screen
        .lines()
        .find(|l| l.contains("Logs") && l.contains("Filter"))
        .expect("logs title row");
    assert!(title.contains('\u{f0c5}'), "back to the clipboard glyph");
}

#[test]
fn logs_records_keep_a_row_of_padding_top_and_bottom() {
    let mut app = app();
    for i in 0..40 {
        app.push_output(&format!("line {i}"));
    }
    // One record long enough to reach the card's edge, so the right padding is measured and
    // not just the blank fill of a short line.
    app.push_output(&"x".repeat(200));
    let screen = render_to_string(&app, 110, 25);
    let lines: Vec<&str> = screen.lines().collect();
    let title = lines
        .iter()
        .position(|line| line.contains("Logs") && line.contains("Filter"))
        .expect("Logs card title");
    let bottom = lines[title + 1..]
        .iter()
        .position(|line| line.starts_with('\u{2570}') && line.ends_with('\u{256f}'))
        .map(|offset| title + 1 + offset)
        .expect("Logs card bottom");
    let blank_inner = |line: &str| {
        let chars: Vec<_> = line.chars().collect();
        chars.len() >= 2
            && chars[0] == '\u{2502}'
            && chars[chars.len() - 1] == '\u{2502}'
            && chars[1..chars.len() - 1]
                .iter()
                .all(|c| matches!(c, ' ' | '\u{2551}' | '\u{2588}'))
    };
    assert!(blank_inner(lines[title + 1]), "top padding row");
    assert!(blank_inner(lines[bottom - 1]), "bottom padding row");
    // Two columns of air on each side: a Nerd Font glyph draws wider than its cell, so a
    // single column reads as no gap at all.
    let first = lines[title + 2];
    assert!(
        first.starts_with("\u{2502}   "),
        "records sit inside the border: {first}"
    );
    // The long record stops two columns before the scrollbar, and the bar keeps a column of its
    // own before the border.
    let long = lines
        .iter()
        .find(|line| line.contains("xxxx"))
        .expect("the long record");
    let cells: Vec<char> = long.chars().collect();
    let bar = cells
        .iter()
        .position(|cell| *cell == '\u{2551}')
        .expect("the scrollbar");
    // The bar takes the padding column, with a blank cell on each side of it.
    assert!(
        cells[bar - 1] == ' ',
        "a column of air between the record and the bar: {long}"
    );
    assert_eq!(
        cells.len() - 1 - bar,
        2,
        "a column of air between the bar and the border: {long}"
    );
}

#[test]
fn log_scrollbar_supports_track_clicks_and_thumb_dragging() {
    let mut app = app();
    for i in 0..100 {
        app.push_output(&format!("log line {i}"));
    }
    let (_, mut app) = crate::app::dashboard::render_to_string_with_state(&app, 110, 25);
    let layout = crate::app::dashboard::Panels::default();
    let bar = app.logs.scrollbar.expect("log scrollbar geometry");

    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: bar.x,
            row: bar.y,
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );
    assert!(
        app.logs.scroll > 0,
        "clicking the top of the track jumps back"
    );

    let (_, mut app) = crate::app::dashboard::render_to_string_with_state(&app, 110, 25);
    let bar = app.logs.scrollbar.expect("updated scrollbar geometry");
    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: bar.x,
            row: bar.thumb_y,
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );
    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: bar.x,
            row: bar.y.saturating_add(bar.height).saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );
    assert_eq!(
        app.logs.scroll, 0,
        "dragging to the bottom resumes follow mode"
    );
}

#[test]
fn keyboard_and_reducer_dispatch_the_same_toggle_commands() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let cases = [
        (
            crate::modules::panels::logs::Command::ToggleLogs.into(),
            KeyCode::Char('l'),
            KeyModifiers::NONE,
        ),
        (
            crate::modules::panels::logs::Command::ToggleRaw.into(),
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        ),
    ];

    for (command, key, modifiers) in cases {
        // Route 1: the key reports a command and the loop reduces it.
        let mut by_key = app();
        let emitted = press(&mut by_key, KeyEvent::new(key, modifiers));
        assert_eq!(emitted, Some(command), "{key:?} means {command:?}");

        // Route 2: the same command applied directly.
        let mut direct = app();
        reduce(&mut direct, Action::Command(command));

        assert_eq!(
            toggles(&by_key),
            toggles(&direct),
            "the key and the command agree for {command:?}",
        );
        assert_ne!(
            toggles(&by_key),
            toggles(&app()),
            "{command:?} actually changed something",
        );
    }
}

#[test]
fn scroll_keys_report_bounded_scroll_commands() {
    let mut state = app();
    for i in 0..30 {
        state.push_output(&format!("line {i}"));
    }
    state.logs.set_log_viewport(7);

    let cases = [
        (
            KeyCode::Up,
            crate::modules::panels::logs::Command::ScrollLogs(1).into(),
        ),
        (
            KeyCode::Down,
            crate::modules::panels::logs::Command::ScrollLogs(-1).into(),
        ),
        (
            KeyCode::PageUp,
            crate::modules::panels::logs::Command::PageLogsUp.into(),
        ),
        (
            KeyCode::PageDown,
            crate::modules::panels::logs::Command::PageLogsDown.into(),
        ),
        (
            KeyCode::End,
            crate::modules::panels::logs::Command::FollowLatest.into(),
        ),
    ];
    for (key, command) in cases {
        assert_eq!(
            crate::app::runtime::handle_key(&mut state, KeyEvent::new(key, KeyModifiers::NONE),),
            Some(command),
            "{key:?} reports {command:?}",
        );
    }
}

#[test]
fn reducer_is_deterministic_and_a_double_toggle_is_the_identity() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let mut once = app();
    let mut twice = app();
    reduce(
        &mut once,
        Action::Command(crate::modules::panels::logs::Command::ToggleLogs.into()),
    );
    reduce(
        &mut twice,
        Action::Command(crate::modules::panels::logs::Command::ToggleLogs.into()),
    );
    assert_eq!(toggles(&once), toggles(&twice), "same action, same state");

    reduce(
        &mut once,
        Action::Command(crate::modules::panels::logs::Command::ToggleLogs.into()),
    );
    assert_eq!(
        toggles(&once),
        toggles(&app()),
        "a double toggle returns to the start without hidden state",
    );
}

#[test]
fn icon_only_copy_control_has_no_extra_inner_trailing_space() {
    let screen = render_to_string(&app(), 120, 25);
    let title = screen
        .lines()
        .find(|line| line.contains("Logs") && line.contains("Filter"))
        .expect("logs title row");
    assert!(
        title.contains("[\u{f0c5}]"),
        "copy is a compact icon button"
    );
    assert!(
        !title.contains("[\u{f0c5} ]"),
        "copy has no extra trailing space"
    );
}

#[test]
fn log_filter_rows_select_on_hover_and_toggle_on_click() {
    let mut state = app();
    state.overlay = Overlay::LogFilter;
    let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 110, 30);
    let row = state.logs.filter_rows[3];

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
    assert_eq!(state.logs.filter_row, 3, "hover selects the warning row");
    assert!(state.logs.filter.kinds.contains(LogKind::Warning));

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
    assert!(!state.logs.filter.kinds.contains(LogKind::Warning));
}

#[test]
fn clicking_the_log_filter_query_focuses_it_for_typing() {
    let mut state = app();
    state.overlay = Overlay::LogFilter;
    let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 110, 30);
    let query = state.logs.filter_rows[0];
    mouse(
        &mut state,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: query.0 + 1,
            row: query.1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );
    assert_eq!(state.logs.filter_row, 0);
    assert!(state.logs.filter_editing);
}

// ---- Every assertion below is on the Logs panel's own state ----

use crate::app::testing::failed_build_json;
use crate::common::nix_events::LogLevel;

#[test]
fn a_multi_line_error_is_one_red_block_instead_of_one_clipped_row() {
    let mut app = app();
    app.ingest_json(&failed_build_json());

    assert_eq!(
        app.logs.entries.len(),
        8,
        "every line of the block is its own record"
    );
    assert!(
        app.logs
            .entries
            .iter()
            .all(|entry| !entry.event.raw().contains('\n')),
        "a record is one terminal row"
    );
    assert!(
        app.logs
            .entries
            .iter()
            .all(|entry| entry.event.level == LogLevel::Error),
        "the whole block reads as the error it belongs to"
    );
    assert!(
        app.logs
            .entries
            .iter()
            .all(|entry| entry.event.kind() == LogKind::Error),
        "and is filtered as an error too"
    );
}

#[test]
fn the_log_filter_keeps_a_cell_of_air_inside_its_border() {
    let mut app = app();
    app.overlay = Overlay::LogFilter;
    let screen = render_to_string(&app, 90, 26);
    let lines: Vec<&str> = screen.lines().collect();

    let top = lines
        .iter()
        .position(|line| line.contains("Filter logs ─"))
        .expect("the filter's top rule");
    let query = lines
        .iter()
        .position(|line| line.contains("query"))
        .expect("its query row");
    assert_eq!(
        query,
        top + 2,
        "the body starts one cell under the rule, from `dialog_body`:\n{screen}"
    );
    let corner = lines[top]
        .chars()
        .position(|cell| cell == '╭')
        .expect("the dialog's left corner");
    let marker = lines[query]
        .chars()
        .position(|cell| cell == '❯')
        .expect("the selected row's marker");
    assert!(
        marker >= corner + 2,
        "and one cell right of the border, so the query never touches it:\n{screen}"
    );
}

#[test]
fn saving_the_filter_names_the_query_and_the_kinds() {
    use crate::common::config::Settings;

    let mut state = crate::modules::panels::logs::State::default();
    state.filter.query = "error".into();
    state.filter.kinds.toggle(LogKind::Info);
    let mut settings = Settings::default();

    let effects = crate::modules::panels::logs::reduce(
        &mut state,
        &mut settings,
        crate::modules::panels::logs::Command::SaveFilter,
    );

    assert_eq!(
        effects,
        vec![crate::modules::panels::logs::Effect::SaveFilter {
            query: "error".into(),
            kinds: "errors,warnings,evaluation,fetch,build,activate,cleanup".into(),
        }],
        "the dialog hands over what it shows, not the whole list of kinds"
    );
    assert!(state.saved, "and the button can say it kept them");
    assert_eq!(
        settings.log_filter_query, None,
        "the frame's own settings are not the file's copy"
    );
}

#[test]
fn a_saved_filter_is_what_the_panel_starts_filtered_by() {
    let filter =
        crate::modules::panels::logs::LogFilter::from_saved(Some("error"), Some("errors,warnings"));

    assert_eq!(filter.query, "error");
    assert!(filter.kinds.contains(LogKind::Error));
    assert!(filter.kinds.contains(LogKind::Warning));
    assert!(
        !filter.kinds.contains(LogKind::Build),
        "the kinds the file left out stay off"
    );
    assert_eq!(
        crate::modules::panels::logs::LogFilter::from_saved(None, None),
        crate::modules::panels::logs::LogFilter::default(),
        "nothing saved leaves the panel as it was"
    );
}
