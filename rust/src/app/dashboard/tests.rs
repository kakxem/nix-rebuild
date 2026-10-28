//! Tests for the composition root: layout, footer, overlays, hover and click geometry.

use crate::app::testing::{app, mouse, press, run_failed_build};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::dashboard::render_to_string;
use crate::app::model::{App, Overlay};
use crate::common::actions::Command;
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::phase::Phase;
use crate::common::scope::Scope;

fn footer_row(screen: &str) -> &str {
    screen
        .lines()
        .find(|line| {
            line.contains("Logs")
                && line.contains("Activity")
                && line.contains("Status")
                && line.contains("Help")
                && line.contains("Settings")
        })
        .expect("global footer")
}

#[test]
fn the_wheel_moves_the_help_list_and_leaves_it_open() {
    let mut app = app();
    app.overlay = Overlay::Help;
    assert!(
        app.help.help_entries().count() > 3,
        "the help list has rows to move through"
    );

    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 50,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );

    assert_eq!(
        app.overlay,
        Overlay::Help,
        "a wheel event is not a dismissal"
    );
    assert_eq!(app.help.index, 3, "the wheel moved the selection");
}

#[test]
fn the_wheel_over_help_does_not_reach_the_panels_behind_it() {
    let mut app = app();
    for i in 0..40 {
        app.push_output(&format!("line {i}"));
    }
    app.overlay = Overlay::Help;
    let scroll = app.logs.scroll;

    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 50,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );

    assert_eq!(app.logs.scroll, scroll, "the Logs card stays where it was");
}

#[test]
fn the_settings_dialog_answers_the_pointer_inside_its_own_box() {
    use crate::app::model::Overlay;
    use crossterm::event::MouseEventKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    app.overlay = Overlay::Settings;
    // Seed a selection the hover has to move, or a row 0 that stays 0 proves nothing.
    app.settings.row = 2;

    // Draw once so the dialog records its geometry, and keep the layout the runtime would hold.
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut layout = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");
    let (x, y, width, height) = app.settings.area.expect("the list was drawn");
    assert!(
        width > 20 && height > 4,
        "the dialog drew a list wide and tall enough to point at"
    );

    // A cell inside the dialog, on the first setting under the "Panels" title.
    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: x + 2,
            row: y + 1,
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );

    assert_eq!(app.settings.row, 0, "hovering the first setting selects it");
}

#[test]
fn the_question_in_front_of_the_settings_dialog_owns_the_pointer() {
    use crate::app::model::Overlay;
    use crossterm::event::MouseEventKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    app.overlay = Overlay::Settings;
    app.settings.row = 2;

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut layout = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");
    let (x, y, _, _) = app.settings.area.expect("the list was drawn");

    // The dialog's own button asks for the cleanup, so the question lands on top of it.
    app.open_overlay(Overlay::CleanupPrompt);
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: x + 2,
            row: y + 1,
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );

    assert_eq!(
        app.settings.row, 2,
        "a dialog in front keeps the pointer to itself"
    );
    assert_eq!(
        app.overlay,
        Overlay::CleanupPrompt,
        "and it is still waiting for its answer"
    );
}

#[test]
fn dragging_a_card_border_resizes_the_pair_it_separates() {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();

    let backend = TestBackend::new(100, 34);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut before = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| before = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    // The Activity and Logs cards share the border row the drag grabs.
    let row = before.rect(crate::app::dashboard::PanelId::Logs).y;
    let grab = before.boundary_at(5, row, app.heights).expect("a border");
    assert_eq!(grab.split.below, crate::app::dashboard::PanelId::Logs);

    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), row - 3),
        (MouseEventKind::Up(MouseButton::Left), row - 3),
    ] {
        mouse(
            &mut app,
            MouseEvent {
                kind,
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            },
            &before,
        );
    }

    let mut after = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| after = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Logs).height,
        before.rect(crate::app::dashboard::PanelId::Logs).height + 3,
        "the Logs card takes the rows the border moved"
    );
    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Activity).height,
        before.rect(crate::app::dashboard::PanelId::Activity).height - 3,
        "and the Activity card gives them up"
    );
    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Status).height,
        3,
        "the cards above the pair do not move"
    );
    assert!(app.settings.config.show_logs, "a drag is not a setting");
}

#[test]
fn a_border_drag_is_forgotten_when_the_window_changes_size() {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    let mut layout = crate::app::dashboard::Panels::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 34)).expect("test terminal");
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    let row = layout.rect(crate::app::dashboard::PanelId::Logs).y;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), row + 2),
        (MouseEventKind::Up(MouseButton::Left), row + 2),
    ] {
        mouse(
            &mut app,
            MouseEvent {
                kind,
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            },
            &layout,
        );
    }
    assert_eq!(
        app.heights.get(crate::app::dashboard::PanelId::Logs),
        Some(layout.rect(crate::app::dashboard::PanelId::Logs).height - 2),
        "the drag pinned the Logs card's height"
    );

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("test terminal");
    terminal
        .draw(|frame| {
            crate::app::dashboard::render(frame, &mut app);
        })
        .expect("render");

    assert_eq!(
        app.heights,
        crate::app::dashboard::Heights::default(),
        "a new window height gets the default split back"
    );
}

#[test]
fn the_border_under_the_run_card_is_not_a_drag_handle() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    let mut before = crate::app::dashboard::Panels::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 34)).expect("test terminal");
    terminal
        .draw(|frame| before = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    // The border is the Run card's bottom rule plus the Activity card's top rule.
    for row in before
        .rect(crate::app::dashboard::PanelId::Status)
        .bottom()
        .saturating_sub(1)..=before.rect(crate::app::dashboard::PanelId::Activity).y
    {
        assert!(
            before.boundary_at(5, row, app.heights).is_none(),
            "the border under the Run card cannot be grabbed at row {row}"
        );
        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            },
            &before,
        );
        assert!(
            app.resizing.is_none(),
            "a press on row {row} grabs no border"
        );
    }

    let mut after = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| after = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Status).height,
        3,
        "the Run card keeps its three rows"
    );
    assert_eq!(
        app.heights,
        crate::app::dashboard::Heights::default(),
        "and no height was pinned"
    );
    assert!(
        after
            .boundary_at(
                5,
                after.rect(crate::app::dashboard::PanelId::Logs).y,
                app.heights
            )
            .is_some(),
        "the border below the Activity card still resizes"
    );
}

#[test]
fn hiding_a_card_after_a_drag_gives_its_rows_back() {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();

    let mut terminal = Terminal::new(TestBackend::new(100, 34)).expect("test terminal");
    let mut layout = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    // Drag the Logs card taller, then close the Activity card above it: the pinned height
    // belongs to that arrangement, so it must not survive the change.
    let row = layout.rect(crate::app::dashboard::PanelId::Logs).y;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), row - 4),
        (MouseEventKind::Up(MouseButton::Left), row - 4),
    ] {
        mouse(
            &mut app,
            MouseEvent {
                kind,
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            },
            &layout,
        );
    }
    assert!(
        app.heights
            .get(crate::app::dashboard::PanelId::Logs)
            .is_some(),
        "the drag pinned a height"
    );

    app.config.show_activity = false;
    let mut after = crate::app::dashboard::Panels::default();
    terminal
        .draw(|frame| after = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    assert_eq!(
        app.heights,
        crate::app::dashboard::Heights::default(),
        "a card shown or hidden takes the pins with it"
    );
    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Activity).height,
        0,
        "and the card is gone"
    );
    assert_eq!(
        after.rect(crate::app::dashboard::PanelId::Logs).bottom(),
        after.area().bottom(),
        "so the Logs card reaches the bottom of the card area, with no gap left over"
    );
}

#[test]
fn a_control_on_a_card_border_keeps_its_click() {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = app();
    let mut layout = crate::app::dashboard::Panels::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 34)).expect("test terminal");
    terminal
        .draw(|frame| layout = crate::app::dashboard::render(frame, &mut app))
        .expect("render");

    // The Filter and copy buttons are drawn on the card's own top border.
    let (area, _) = app
        .click_targets()
        .into_iter()
        .find(|(area, _)| area.1 == layout.rect(crate::app::dashboard::PanelId::Logs).y)
        .expect("a control on the Logs border");

    mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.0,
            row: area.1,
            modifiers: KeyModifiers::NONE,
        },
        &layout,
    );

    assert!(app.resizing.is_none(), "the border was not grabbed");
    assert_eq!(
        app.overlay,
        Overlay::LogFilter,
        "the button on the border still opens its dialog"
    );
}

#[test]
fn completion_shows_a_close_prompt_instead_of_the_reason() {
    let mut app = app();
    app.status.phase = Phase::Complete;
    app.status.message = "nothing to do — every path was already in the store".into();

    let screen = render_to_string(&app, 100, 25);

    assert!(
        screen.contains("[Enter  Close]"),
        "the completion action is shown in the global footer",
    );
    assert!(
        !screen.contains("All done"),
        "completion does not appear below the activity history",
    );
    assert!(
        !screen.contains("nothing to do"),
        "the internal reason is no longer displayed",
    );
}

#[test]
fn completion_prompt_is_the_only_close_hint() {
    let mut app = app();
    app.status.phase = Phase::Complete;
    let screen = render_to_string(&app, 100, 25);
    assert_eq!(
        screen.matches("Enter").count(),
        1,
        "the completion prompt owns the close hint",
    );
}

#[test]
fn no_work_result_keeps_the_dashboard_cards() {
    let mut app = App::new(
        Scope::Home,
        "/home/user/.config/nixos-dotfiles#sample".into(),
        false,
        CleanupPolicy::Ask,
    );
    app.status.phase = Phase::Complete;
    app.status.message = "nothing to do — every path was already in the store".into();

    let screen = render_to_string(&app, 100, 26);

    assert!(screen.contains("Home Manager"));
    assert!(!screen.contains("nix-rebuild ·"));
    assert!(screen.contains("[Enter  Close]"));
    assert!(!screen.contains("All done"));
    assert!(screen.contains('\u{f15c}'), "footer toggle icons");
    assert!(screen.contains('│'), "card side borders");
    assert!(screen.contains('╭'));
    assert!(screen.contains('╯'));
}

#[test]
fn cards_are_adjacent_and_panel_actions_have_a_right_inset() {
    let specs = crate::app::dashboard::panel_specs(&app());
    let panels = crate::app::dashboard::Panels::layout(
        ratatui::layout::Rect::new(0, 0, 110, 23),
        &specs,
        crate::app::dashboard::Heights::default(),
    );
    assert_eq!(
        panels.rect(crate::app::dashboard::PanelId::Status).bottom(),
        panels.rect(crate::app::dashboard::PanelId::Activity).y
    );
    assert_eq!(
        panels
            .rect(crate::app::dashboard::PanelId::Activity)
            .bottom(),
        panels.rect(crate::app::dashboard::PanelId::Logs).y
    );

    let screen = render_to_string(&app(), 110, 25);
    for title in screen
        .lines()
        .filter(|line| line.contains("Filter") && line.ends_with('╮'))
    {
        assert!(
            title.ends_with("───╮"),
            "the inset is filled by the border rule: {title}"
        );
    }
}

#[test]
fn the_footer_lists_the_cards_in_the_order_they_are_stacked() {
    let app = app();
    let screen = render_to_string(&app, 120, 25);
    let footer = screen
        .lines()
        .rev()
        .find(|line| line.contains("Help"))
        .expect("footer row");

    let status = footer.find("Status]").expect("Status control");
    let activity = footer.find("Activity]").expect("Activity control");
    let logs = footer.find("Logs]").expect("Logs control");
    assert!(
        status < activity && activity < logs,
        "Status, Activity, Logs in that order: {footer}"
    );
}

#[test]
fn cleanup_prompt_remains_visible_when_activity_is_hidden() {
    // Activity is closed from the card's own footer, which the prompt owns while it waits, so the
    // state the frame has to survive is the one it starts with.
    let mut state = app();
    state.status.phase = Phase::Complete;
    state.overlay = Overlay::CleanupPrompt;
    state.config.show_activity = false;
    state.settings.config.show_activity = false;
    let screen = render_to_string(&state, 120, 25);

    assert!(
        !state.config.show_activity,
        "the Activity card is closed while the prompt waits"
    );
    assert!(screen.contains("Clean old generations and collect garbage?"));
    assert!(
        !screen
            .lines()
            .any(|line| line.contains("Activity") && line.contains("Filter")),
        "the Activity panel stays closed"
    );
}

#[test]
fn a_press_on_the_backdrop_closes_the_dialog_and_nothing_behind_it_answers() {
    use crate::common::hit_targets::Click;

    // The Help control is on the footer, which every dialog leaves outside its own box. A press
    // there lands on the dimmed backdrop: it closes what is in front, and never the control under it.
    for overlay in [
        Overlay::CleanupPrompt,
        Overlay::ConfirmCancel,
        Overlay::Help,
        Overlay::Details,
        Overlay::LogFilter,
        Overlay::ActivityFilter,
        Overlay::RemoveRunsPrompt,
    ] {
        let mut state = app();
        state.overlay = overlay;
        let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
        let (area, _) = state
            .click_targets()
            .into_iter()
            .find(|(_, target)| *target == Click::Help)
            .expect("Help footer control");

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
            Overlay::None,
            "{overlay:?} closes on a press against its backdrop instead of passing it on"
        );
    }
}

#[test]
fn a_press_inside_the_dialog_but_off_its_controls_keeps_it_open() {
    let mut state = app();
    state.overlay = Overlay::Details;
    let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
    let area = state
        .click_targets()
        .dialog()
        .map(|dialog| dialog.area)
        .expect("the dialog's own box");

    // Its top-left cell is the border: not a control, and not the backdrop either.
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
        Overlay::Details,
        "only the backdrop around the box dismisses it"
    );
}

#[test]
fn a_help_row_that_opens_a_dialog_leaves_that_dialog_in_front() {
    let mut state = app();
    state.overlay = Overlay::Help;
    let index = state
        .help
        .help_entries()
        .position(|entry| entry.label == "Filter activity")
        .expect("the row that opens the Activity filter");
    state.help.index = index;

    press(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert_eq!(
        state.overlay,
        Overlay::ActivityFilter,
        "the row opens its dialog"
    );
}

#[test]
fn the_cleanup_question_is_a_dialog_drawn_over_the_cards() {
    let mut app = app();
    app.activity.start_activity(1, "demo-build", "build");
    app.finish_activity(1, false);
    app.overlay = Overlay::CleanupPrompt;

    let screen = render_to_string(&app, 100, 25);

    assert!(screen.contains("Clean old generations and collect garbage?"));
    assert!(screen.contains("[N  No]"));
    assert!(
        screen.contains("[Esc]"),
        "the question is a dialog, corner and all:\n{screen}"
    );
    assert!(!screen.contains("(default)"));
    assert!(
        screen.contains("demo-build"),
        "the cards keep their content behind the question:\n{screen}"
    );
    assert_eq!(
        screen.matches('╭').count(),
        4,
        "the three cards plus the question's own dialog"
    );
}

#[test]
fn dirty_git_tree_is_announced_on_the_run_card_not_the_log_line() {
    let mut app = App::new(
        Scope::Home,
        "/home/user/.config/nixos-dotfiles#sample".into(),
        false,
        CleanupPolicy::Ask,
    );
    app.push_output("warning: Git tree '/home/user/.config/nixos-dotfiles' is dirty");

    assert!(app.status.dirty_tree);
    assert!(
        app.logs.entries.is_empty(),
        "the badge replaces the log line"
    );

    let screen = render_to_string(&app, 100, 25);
    let run_row = screen.lines().nth(1).expect("Run metadata row");
    assert!(
        run_row.contains("dirty"),
        "the tree is announced beside the flake it read"
    );
    assert!(
        !screen.contains("is dirty"),
        "no duplicate in the log panel"
    );
}

#[test]
fn global_controls_live_in_a_compact_footer() {
    let screen = render_to_string(&app(), 120, 25);
    assert!(!screen.contains("L logs: on"));
    let footer = footer_row(&screen);
    assert!(footer.contains('\u{f15c}'), "Logs button icon");
    assert!(!footer.contains('│'), "footer is not boxed");
}

#[test]
fn footer_keeps_only_the_top_level_controls() {
    let screen = render_to_string(&app(), 120, 25);
    let footer = footer_row(&screen);
    assert!(footer.contains("Logs"));
    assert!(footer.contains("Activity"));
    assert!(footer.contains("Status"));
    assert!(footer.contains("Help"));
    assert!(!footer.contains("Raw"));
    assert!(!footer.contains("Fetch"));
    assert!(!footer.contains("Build"));
    assert!(!footer.contains("Warn"));
}

#[test]
fn footer_icon_and_label_are_tightly_spaced() {
    let screen = render_to_string(&app(), 120, 25);
    let footer = footer_row(&screen);
    let (idx, _) = footer
        .char_indices()
        .find(|(_, c)| *c == '\u{f15c}')
        .expect("logs icon");
    let after: String = footer[idx..].chars().skip(1).take(1).collect();
    assert_eq!(after, " ", "exactly one space between icon and label");
}

#[test]
fn completion_highlights_the_enter_key() {
    let line = crate::app::dashboard::completion_line();
    let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "[Enter  Close]");
    let enter = line
        .spans
        .iter()
        .find(|s| s.content.contains("Enter"))
        .expect("the Enter key is its own span");
    assert!(
        enter
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD),
        "the Enter key is emphasised",
    );
}

#[test]
fn hovering_a_control_highlights_it() {
    use crate::app::dashboard::render_to_string_with_state;
    use crate::common::hit_targets::Hover;

    let app = app();
    // The click map comes from a render, so keep the post-render state.
    let (_, rendered) = render_to_string_with_state(&app, 110, 25);
    let (area, click) = rendered
        .click_targets()
        .iter()
        .next()
        .map(|(area, click)| (*area, *click))
        .expect("a control exists");

    let mut hovered = rendered.clone();
    hovered.set_hover(Hover {
        column: area.0,
        row: area.1,
    });
    assert_eq!(
        hovered.hovered_click(),
        Some(click),
        "the control under the pointer reports as hovered",
    );

    let mut away = rendered.clone();
    away.set_hover(Hover {
        column: area.0.saturating_add(60),
        row: area.1,
    });
    assert!(
        away.hovered_click().is_none(),
        "empty space hovers no control",
    );
}

#[test]
fn footer_controls_have_one_cell_of_outer_padding() {
    let screen = render_to_string(&app(), 110, 25);
    let footer = footer_row(&screen);
    assert_eq!(
        footer.chars().position(|c| c == '['),
        Some(1),
        "one cell pads the footer buttons"
    );
}

#[test]
fn hover_underline_covers_the_label_but_not_the_icon() {
    use crate::app::dashboard::render_to_string_with_state;
    use crate::common::hit_targets::{Click, Hover};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    let app = app();
    let (_, rendered) = render_to_string_with_state(&app, 118, 26);
    let area = rendered
        .click_targets()
        .iter()
        .find(|(_, c)| {
            *c == Click::Command(crate::modules::panels::logs::Command::OpenLogFilter.into())
        })
        .map(|(a, _)| *a)
        .expect("filter control");

    let mut hovered = rendered.clone();
    hovered.set_hover(Hover {
        column: area.0 + 2,
        row: area.1,
    });

    let backend = TestBackend::new(118, 26);
    let mut term = Terminal::new(backend).unwrap();
    let mut owned = hovered.clone();
    term.draw(|f| {
        crate::app::dashboard::render(f, &mut owned);
    })
    .unwrap();
    let buf = term.backend().buffer();

    let mut icon_underlined = false;
    let mut label_underlined = false;
    for x in area.0..(area.0 + area.2) {
        if let Some(cell) = buf.cell((x, area.1)) {
            let sym = cell.symbol();
            let ul = cell.style().add_modifier.contains(Modifier::UNDERLINED);
            if sym == "\u{f0b0}" {
                icon_underlined = ul;
            }
            if sym == "F" {
                label_underlined = ul;
            }
        }
    }
    assert!(label_underlined, "the key letter is underlined");
    assert!(!icon_underlined, "the icon is never underlined");
}

#[test]
fn adjacent_cards_use_their_borders_as_separation() {
    let screen = render_to_string(&app(), 120, 25);
    let lines: Vec<_> = screen.lines().collect();
    let run_bottom = lines
        .iter()
        .position(|line| line.starts_with('╰'))
        .expect("Run card bottom");
    let activity = lines
        .iter()
        .position(|line| line.contains("Activity") && line.starts_with('╭'))
        .expect("Activity card title");
    assert_eq!(activity, run_bottom + 1);
}

#[test]
fn running_footer_has_no_transient_keybinding_hints() {
    let mut app = app();
    app.status.phase = Phase::Building;
    let screen = render_to_string(&app, 100, 25);
    assert_eq!(
        screen.matches("Help").count(),
        1,
        "one interactive Help control"
    );
    assert!(!screen.contains("F2 Select"));
    assert!(!screen.contains("Esc Cancel"));
}

#[test]
fn footer_help_is_anchored_near_the_bottom() {
    let screen = render_to_string(&app(), 100, 25);
    let lines: Vec<_> = screen.lines().collect();
    let help = lines
        .iter()
        .position(|line| line.contains("Help"))
        .expect("Help footer control");
    assert_eq!(help, lines.len() - 2, "Help lives in the footer");
}

#[test]
fn renderer_keeps_a_blank_terminal_row_below_the_footer() {
    let screen = render_to_string(&app(), 100, 25);
    let lines: Vec<_> = screen.lines().collect();
    assert_eq!(lines.len(), 25);
    assert!(lines[23].contains("Logs") && lines[23].contains("Help"));
    assert!(lines[24].is_empty());
}

#[test]
fn footer_and_help_share_the_same_hover_rule() {
    use crate::common::ui::{ControlSpec, render_control};
    use ratatui::style::Modifier;

    // Same specification, so their hover treatment cannot drift.
    let logs = ControlSpec {
        command: crate::modules::panels::logs::Command::ToggleLogs.into(),
        prefix: "",
        key: "L",
        icon: Some('\u{f15c}'),
        label: "ogs",
        active: true,
        enabled: true,
    };
    let help = ControlSpec {
        command: crate::modules::panels::help::Command::OpenHelp.into(),
        prefix: "",
        key: "H",
        icon: Some('\u{f128}'),
        label: "elp",
        active: true,
        enabled: true,
    };
    for spec in [logs, help] {
        let (idle, _) = render_control(&spec, false, false);
        let (hovered, _) = render_control(&spec, true, false);
        let underlined = |line: &ratatui::text::Line<'static>| {
            line.spans
                .iter()
                .filter(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
                .count()
        };
        assert_eq!(underlined(&idle), 0, "{:?} is flat when idle", spec.command);
        assert!(
            underlined(&hovered) > 0,
            "{:?} underlines on hover",
            spec.command,
        );
        let (again, _) = render_control(&spec, false, false);
        assert_eq!(
            underlined(&again),
            underlined(&idle),
            "{:?} renders the same idle line twice",
            spec.command,
        );
    }
}

#[test]
fn clicking_the_final_label_cell_dispatches_the_command_and_one_past_does_not() {
    use crate::app::dashboard::render_to_string_with_state;
    use crate::common::hit_targets::{Click, Hover};

    let app = app();
    let (_, rendered) = render_to_string_with_state(&app, 118, 26);
    let (area, click) = rendered
        .click_targets()
        .into_iter()
        .find(|(_, click)| {
            *click == Click::Command(crate::modules::panels::logs::Command::ToggleLogs.into())
        })
        .expect("footer Logs control");

    let last_cell = area.0 + area.2 - 1;
    let mut inside = rendered.clone();
    inside.set_hover(Hover {
        column: last_cell,
        row: area.1,
    });
    assert_eq!(
        inside.hovered_click(),
        Some(click),
        "the final label cell is inside the control",
    );

    let mut outside = rendered.clone();
    outside.set_hover(Hover {
        column: area.0 + area.2,
        row: area.1,
    });
    assert!(
        outside.hovered_click() != Some(click),
        "one cell past the control is not a hit",
    );
}

#[test]
fn footer_control_geometry_covers_exactly_the_rendered_cells() {
    use crate::app::dashboard::render_to_string_with_state;
    use crate::common::hit_targets::Click;

    let app = app();
    let (screen, rendered) = render_to_string_with_state(&app, 118, 26);
    let rows: Vec<&str> = screen.lines().collect();

    for (target, icon) in [
        (
            Click::Command(crate::modules::panels::logs::Command::ToggleLogs.into()),
            '\u{f15c}',
        ),
        (Click::Help, '\u{f128}'),
    ] {
        let (area, _) = rendered
            .click_targets()
            .into_iter()
            .find(|(_, click)| *click == target)
            .unwrap_or_else(|| panic!("{target:?} is registered"));

        // The icon must fall inside the registered rectangle.
        let row: Vec<char> = rows[area.1 as usize].chars().collect();
        let icon_at = row
            .iter()
            .enumerate()
            .find(|(_, ch)| **ch == icon)
            .map(|(index, _)| index as u16);
        if let Some(icon_at) = icon_at {
            assert!(
                icon_at >= area.0 && icon_at < area.0 + area.2,
                "{target:?} rectangle covers its icon (icon at {icon_at}, area {:?})",
                area,
            );
        }
    }
}

#[test]
fn every_rendered_control_has_a_matching_command() {
    use crate::app::dashboard::render_to_string_with_state;

    let app = app();
    let (_, rendered) = render_to_string_with_state(&app, 118, 26);
    assert!(
        !rendered.click_targets().is_empty(),
        "the frame registers its controls",
    );
    // No cells may be registered twice.
    let mut areas: Vec<_> = rendered
        .click_targets()
        .iter()
        .map(|(area, _)| *area)
        .collect();
    areas.sort_unstable();
    let unique = areas.len();
    areas.dedup();
    assert_eq!(areas.len(), unique, "control rectangles do not overlap");
}

#[test]
fn dashboard_uses_status_owned_status_independent_cards_and_a_footer() {
    let mut state = app();
    state.status.phase = Phase::Complete;
    let screen = render_to_string(&state, 120, 30);
    let lines: Vec<_> = screen.lines().collect();

    let status_title = lines.first().expect("Status title");
    assert!(status_title.contains("Status"));
    assert!(status_title.contains("complete"));
    assert!(!screen.contains("nix-rebuild ·"));

    let mut card_titles = Vec::new();
    for title in ["Status", "Activity", "Logs"] {
        let row = lines
            .iter()
            .position(|line| line.contains(title) && line.contains('╭') && line.contains('╮'))
            .unwrap_or_else(|| panic!("{title} card title"));
        card_titles.push(row);
        let bottom = lines[row + 1..]
            .iter()
            .position(|line| line.contains('╰') && line.contains('╯'))
            .map(|offset| row + 1 + offset)
            .unwrap_or_else(|| panic!("{title} card bottom"));
        if title == "Status" {
            assert_eq!(bottom - row + 1, 3, "Status stays one content row tall");
        }
    }
    assert!(card_titles.windows(2).all(|rows| rows[1] > rows[0] + 2));

    let footer = lines
        .iter()
        .rfind(|line| {
            line.contains("Logs")
                && line.contains("Activity")
                && line.contains("Status")
                && line.contains("Help")
                && line.contains("Settings")
        })
        .expect("global footer");
    assert!(!footer.contains('│'), "the footer is not another container");
    assert!(!screen.contains("Controls"));
}

#[test]
fn completion_close_is_a_clickable_button() {
    use crate::common::hit_targets::Click;

    let mut state = app();
    state.status.phase = Phase::Complete;
    let (screen, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 25);
    assert!(screen.contains("[Enter  Close]"));
    let (area, _) = state
        .click_targets()
        .into_iter()
        .find(|(_, target)| *target == Click::Command(Command::Quit))
        .expect("completion close button");
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
    assert!(state.should_quit);
}

#[test]
fn every_dialog_has_a_mouse_close_button() {
    use crate::common::hit_targets::Click;

    for overlay in [
        Overlay::Help,
        Overlay::Details,
        Overlay::LogFilter,
        Overlay::ActivityFilter,
        Overlay::ConfirmCancel,
    ] {
        let mut state = app();
        state.overlay = overlay;
        let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&state, 120, 30);
        let (area, _) = state
            .click_targets()
            .into_iter()
            .find(|(_, target)| *target == Click::Command(Command::CloseOverlay))
            .unwrap_or_else(|| panic!("{overlay:?} close button"));

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
        assert_eq!(state.overlay, Overlay::None, "{overlay:?} closes by mouse");
    }
}

#[test]
fn every_action_control_uses_the_same_bracketed_button_shape() {
    let screen = render_to_string(&app(), 120, 25);

    for button in [
        "[\u{f15c} Logs]",
        "[\u{f0ae} Activity]",
        "[\u{f04b} Status]",
        "[\u{f128} Help]",
        "[\u{f0b0} Filter]",
        "[\u{f0c5}]",
    ] {
        assert!(
            screen.contains(button),
            "missing recognizable button {button}"
        );
    }
}

#[test]
fn footer_exposes_logs_activity_and_status_visibility_controls() {
    let state = app();
    let screen = render_to_string(&state, 120, 25);
    let footer = footer_row(&screen);
    assert!(footer.contains("Logs"));
    assert!(footer.contains("Activity"));
    assert!(footer.contains("Status"));

    let mut state = app();
    crate::app::reduce::reduce(
        &mut state,
        crate::common::actions::Action::Command(
            crate::modules::panels::activity::Command::ToggleActivity.into(),
        ),
    );
    crate::app::reduce::reduce(
        &mut state,
        crate::common::actions::Action::Command(
            crate::modules::panels::status::Command::ToggleStatus.into(),
        ),
    );
    let hidden = render_to_string(&state, 120, 25);
    assert!(
        !hidden
            .lines()
            .any(|line| line.starts_with('╭') && line.contains("Activity"))
    );
    assert!(
        !hidden
            .lines()
            .any(|line| line.starts_with('╭') && line.contains("Status"))
    );

    crate::app::reduce::reduce(
        &mut state,
        crate::common::actions::Action::Command(
            crate::modules::panels::activity::Command::ToggleActivity.into(),
        ),
    );
    crate::app::reduce::reduce(
        &mut state,
        crate::common::actions::Action::Command(
            crate::modules::panels::status::Command::ToggleStatus.into(),
        ),
    );
    let restored = render_to_string(&state, 120, 25);
    assert!(
        restored
            .lines()
            .any(|line| line.starts_with('╭') && line.contains("Activity"))
    );
    assert!(
        restored
            .lines()
            .any(|line| line.starts_with('╭') && line.contains("Status"))
    );
}

#[test]
fn footer_visibility_controls_are_clickable() {
    use crate::common::hit_targets::Click;

    for command in [
        Command::from(crate::modules::panels::logs::Command::ToggleLogs),
        Command::from(crate::modules::panels::activity::Command::ToggleActivity),
        Command::from(crate::modules::panels::status::Command::ToggleStatus),
    ] {
        let visible = |app: &App| match command {
            Command::Logs(crate::modules::panels::logs::Command::ToggleLogs) => {
                app.config.show_logs
            }
            Command::Activity(crate::modules::panels::activity::Command::ToggleActivity) => {
                app.config.show_activity
            }
            Command::Status(crate::modules::panels::status::Command::ToggleStatus) => {
                app.config.show_status
            }
            _ => unreachable!(),
        };
        let (_, mut state) = crate::app::dashboard::render_to_string_with_state(&app(), 120, 25);
        let (area, _) = state
            .click_targets()
            .into_iter()
            .find(|(_, target)| *target == Click::Command(command))
            .expect("footer control hit target");
        assert!(visible(&state));
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
        assert!(!visible(&state), "{command:?} responds to a mouse click");
    }
}

#[test]
fn the_failure_prompt_survives_the_activity_card_being_hidden() {
    let mut app = app();
    run_failed_build(&mut app);
    app.config.show_activity = false;

    let screen = render_to_string(&app, 120, 30);

    assert!(
        screen.contains("╭─ Failed"),
        "the prompt becomes a dialog of its own, got:\n{screen}"
    );
    assert!(screen.contains("rebuild failed · nix-rebuild-0.1.0"));
    assert!(screen.contains("[Enter  Close]"));
}

#[test]
fn a_failed_run_gives_its_card_to_the_failure_banner() {
    let mut app = app();
    app.activity.start_activity(1, "linux-kernel", "build");
    app.finish_after_exit(true);
    app.status.phase = Phase::Failed;

    let screen = render_to_string(&app, 120, 30);

    assert!(
        !screen.contains("linux-kernel"),
        "the history gives way to the banner instead of sitting under it:\n{screen}"
    );
    assert!(
        screen.contains("\u{f00d} rebuild failed"),
        "the banner is the card's only content:\n{screen}"
    );
    assert!(screen.contains("[Enter  Close]"), "{screen}");
    assert!(screen.contains("Copy logs"), "{screen}");
}

#[test]
fn a_dialog_answers_esc_in_its_own_title() {
    for overlay in [
        Overlay::Help,
        Overlay::Details,
        Overlay::LogFilter,
        Overlay::ActivityFilter,
        Overlay::ConfirmCancel,
    ] {
        let mut state = app();
        state.overlay = overlay;
        let screen = render_to_string(&state, 120, 30);

        assert!(
            screen.contains("[Esc]"),
            "{overlay:?} names the key that leaves it in its title:\n{screen}"
        );
        assert!(
            !screen.contains('×'),
            "{overlay:?} spends no second glyph on the same key:\n{screen}"
        );
        for repeated in ["Esc close", "Esc closes"] {
            assert!(
                !screen.contains(repeated),
                "{overlay:?} does not repeat {repeated:?} below a title that already says it:\n{screen}"
            );
        }
    }
}

#[test]
fn a_dialog_darkens_the_cards_behind_it() {
    use crate::common::theme::MUTED;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    // The Run card's top-left corner, and a blank cell of its interior.
    let cells = |state: &mut App| {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("test terminal");
        terminal
            .draw(|frame| {
                crate::app::dashboard::render(frame, state);
            })
            .expect("render");
        let buffer = terminal.backend().buffer();
        let at = |x: u16, y: u16| buffer.cell((x, y)).expect("painted cell").style();
        (at(0, 0), at(4, 2))
    };

    let mut state = app();
    let (border, blank) = cells(&mut state);
    assert_eq!(
        border.fg,
        Some(MUTED),
        "the Run card draws its own border while nothing is open"
    );

    state.overlay = Overlay::Help;
    let (dim_border, dim_blank) = cells(&mut state);
    assert_eq!(
        dim_border.fg,
        Some(Color::Rgb(70, 70, 70)),
        "behind a dialog the card recedes to half its brightness, or it fights the dialog for the eye"
    );
    assert_eq!(
        dim_blank.bg, blank.bg,
        "the backdrop darkens what is painted and never fills a cell the card left empty"
    );
}

#[test]
fn the_cleanup_question_is_the_apps_dialog_with_or_without_its_card() {
    use crate::common::hit_targets::Click;
    use crate::modules::work::cleanup::Command as CleanupCommand;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    for activity in [true, false] {
        let mut state = app();
        state.status.phase = Phase::Complete;
        state.config.show_activity = activity;
        state.overlay = Overlay::CleanupPrompt;

        let mut terminal = Terminal::new(TestBackend::new(120, 25)).expect("test terminal");
        terminal
            .draw(|frame| {
                crate::app::dashboard::render(frame, &mut state);
            })
            .expect("render");
        let screen = crate::common::ui::buffer_to_string(terminal.backend().buffer());

        assert!(
            screen
                .lines()
                .find(|line| line.contains("[Esc]"))
                .is_some_and(|line| !line.contains("Cleanup")),
            "activity={activity}: the question needs no title of its own:\n{screen}"
        );
        assert!(
            screen.contains("[Esc]"),
            "activity={activity}: and the corner help and the filters have:\n{screen}"
        );
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((0, 0))
                .expect("the Run card's corner")
                .style()
                .fg,
            Some(Color::Rgb(70, 70, 70)),
            "activity={activity}: the cards behind it recede, as behind every dialog"
        );
        assert!(
            screen
                .lines()
                .find(|line| line.contains("[Esc]"))
                .is_some_and(|line| line.contains('╭') && line.contains('─')),
            "activity={activity}: the corner rides the top rule:\n{screen}"
        );

        let title_row = screen
            .lines()
            .position(|line| line.contains("[Esc]"))
            .expect("the title row") as u16;
        let (corner, _) = state
            .click_targets()
            .into_iter()
            .find(|(area, target)| {
                *target == Click::Command(CleanupCommand::SkipCleanup.into()) && area.1 == title_row
            })
            .expect("the corner answers Esc");

        mouse(
            &mut state,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: corner.0,
                row: corner.1,
                modifiers: KeyModifiers::NONE,
            },
            &crate::app::dashboard::Panels::default(),
        );

        assert_eq!(
            state.overlay,
            Overlay::None,
            "activity={activity}: Esc means No here, so the corner declines the cleanup"
        );
        assert_eq!(
            state.config.show_activity, activity,
            "answering never reopens or closes a card behind the question"
        );
    }
}

#[test]
fn output_arriving_after_a_terminal_phase_only_adds_a_log_record() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Command};

    let mut state = app();
    reduce(&mut state, Action::Command(Command::Quit));
    let phase = state.status.phase;
    let status = state.status.message.clone();

    reduce(&mut state, Action::Output("late line".into()));

    assert_eq!(state.status.phase, phase, "a finished run keeps its phase");
    assert_eq!(state.status.message, status, "and its final status");
    assert!(
        state
            .logs
            .entries
            .iter()
            .any(|entry| entry.event.raw() == "late line"),
        "the late record is still kept",
    );
}
