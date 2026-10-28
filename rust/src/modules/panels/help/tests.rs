//! The help overlay's tests: the selection that moves, the search that narrows, and the entries it documents.

use crate::app::testing::{app, press};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::model::Overlay;
use crate::common::actions::Command;
use crate::common::ui::HelpEntry;

/// A help row seeded here, so the overlay is exercised without naming any sibling slice.
fn close_frame_entry() -> HelpEntry {
    HelpEntry {
        key: "Q",
        label: "Close frame",
        command: Some(Command::Quit),
    }
}

#[test]
fn help_list_moves_with_the_arrows_and_closes_with_escape() {
    let mut app = app();
    app.overlay = Overlay::Help;
    assert_eq!(app.help.index, 0);

    press(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.help.index, 1);

    press(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.help.index, 0);

    press(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::None, "Esc closes the dialog");
}

#[test]
fn help_filter_is_entered_with_slash_and_narrows_the_list() {
    let mut app = app();
    app.overlay = Overlay::Help;
    app.help.entries = vec![
        HelpEntry {
            key: "S",
            label: "Scroll down",
            command: None,
        },
        close_frame_entry(),
    ];

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    assert!(app.help.filtering, "slash starts a search");
    assert_eq!(app.help.index, 0, "selection resets when the query changes");

    for c in "scro".chars() {
        press(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.help.query, "scro");
    let matches: Vec<_> = app
        .help
        .help_entries()
        .map(|entry| entry.label.to_owned())
        .collect();
    assert_eq!(
        matches,
        ["Scroll down"],
        "only the row matching the query remains",
    );
}

#[test]
fn help_selection_survives_filtering_bounds() {
    let mut app = app();
    app.overlay = Overlay::Help;
    for _ in 0..50 {
        press(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let len = app.help.help_entries().count();
    assert_eq!(
        app.help.index,
        len.saturating_sub(1),
        "selection clamps at the end"
    );
}

#[test]
fn help_enter_runs_the_selected_action() {
    let mut app = app();
    app.overlay = Overlay::Help;
    app.help.entries = vec![close_frame_entry()];
    assert_eq!(app.help.index, 0);

    press(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.should_quit, "Enter executed the selected row's command");
    assert_eq!(app.overlay, Overlay::None, "the dialog closed after acting");
}

#[test]
fn help_enter_does_not_execute_while_filtering() {
    let mut app = app();
    app.overlay = Overlay::Help;
    app.help.entries = vec![close_frame_entry()];
    app.help.filtering = true;

    press(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!app.should_quit, "Enter only applies the pending filter");
    assert_eq!(app.overlay, Overlay::Help);
}

#[test]
fn help_enter_dispatches_the_selected_rows_command() {
    let entry = close_frame_entry();

    let mut via_help = app();
    via_help.overlay = Overlay::Help;
    via_help.help.entries = vec![entry];
    via_help.help.index = 0;
    let emitted = press(
        &mut via_help,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert_eq!(
        emitted,
        Some(Command::Quit),
        "Enter on the row runs its command",
    );

    let mut direct = app();
    crate::app::reduce::reduce(
        &mut direct,
        crate::common::actions::Action::Command(entry.command.unwrap()),
    );
    assert_eq!(
        via_help.should_quit, direct.should_quit,
        "the menu and a direct dispatch leave the same state",
    );
}

#[test]
fn every_reported_command_matches_its_documented_key() {
    // The Help list is the contract the user reads: each row's key must produce the row's command.
    for entry in app().help.entries {
        let Some(command) = entry.command else {
            continue;
        };
        let Some(ch) = entry.key.chars().next() else {
            continue;
        };
        if entry.key.chars().count() != 1 {
            continue;
        }
        let mut state = app();
        assert_eq!(
            crate::app::runtime::handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            ),
            Some(command),
            "Help row {:?} documents {ch}, which must dispatch {command:?}",
            entry.label,
        );
    }
}

#[test]
fn closing_an_overlay_is_a_command_that_resets_the_search() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Command};

    let mut state = app();
    state.overlay = Overlay::Help;
    state.help.query = "scro".into();
    state.help.filtering = true;

    reduce(&mut state, Action::Command(Command::CloseOverlay));

    assert_eq!(state.overlay, Overlay::None);
    assert!(
        state.help.query.is_empty(),
        "closing the dialog clears the pending search",
    );
    assert!(!state.help.filtering);
}

// ---- Contract tests ----

use crate::app::dashboard::render_to_string;
use crate::app::testing::{click, mouse};
use crossterm::event::{MouseEvent, MouseEventKind};

#[test]
fn help_dialog_height_tracks_its_content() {
    let mut app = app();
    app.overlay = Overlay::Help;
    let screen = render_to_string(&app, 110, 30);
    // Character cells, not bytes: a box-drawing corner is one cell and three bytes.
    let rows: Vec<Vec<char>> = screen.lines().map(|line| line.chars().collect()).collect();
    let top = rows
        .iter()
        .position(|row| row.iter().collect::<String>().contains("Keyboard"))
        .expect("Help top");
    // The dialog floats over the cards, so its corner is the last one on the title row, not the first.
    let left = rows[top]
        .iter()
        .rposition(|cell| *cell == '╭')
        .expect("Help left");
    let bottom = rows[top + 1..]
        .iter()
        .position(|row| row.get(left) == Some(&'╰'))
        .map(|offset| top + 1 + offset)
        .expect("Help bottom");
    assert_eq!(bottom - top + 1, app.help.help_entries().count() + 5);
}

#[test]
fn the_help_list_is_padded_from_the_title_rule() {
    use crate::app::dashboard::render_to_string_with_state;

    let mut app = app();
    app.overlay = Overlay::Help;
    let (screen, rendered) = render_to_string_with_state(&app, 110, 30);
    let (x, y, width, _) = rendered.help.area.expect("help geometry");
    let row: Vec<char> = screen
        .lines()
        .nth(y as usize - 1)
        .expect("the row between the title and the list")
        .chars()
        .collect();

    assert!(
        row[x as usize..(x + width) as usize]
            .iter()
            .all(|cell| *cell == ' '),
        "the list starts one blank row below the title, like the Logs card"
    );
}

#[test]
fn help_overlay_is_a_navigable_list() {
    let mut app = app();
    app.overlay = Overlay::Help;
    let screen = render_to_string(&app, 100, 25);
    assert!(screen.contains("Keyboard"), "title");
    assert!(screen.contains('\u{276f}'), "a selection cursor is drawn");
    let first = app.help.entries[0].label;
    assert!(screen.contains(first), "first entry listed");
}

#[test]
fn help_footer_control_opens_the_dialog() {
    use crate::common::hit_targets::Click;
    let mut app = app();
    click(&mut app, Click::Help);
    assert_eq!(app.overlay, Overlay::Help);
}

#[test]
fn clicking_a_help_row_selects_and_runs_it() {
    use crate::app::dashboard::render_to_string_with_state;
    let mut app = app();
    app.overlay = Overlay::Help;
    app.help.entries = vec![
        close_frame_entry(),
        HelpEntry {
            key: "X",
            label: "Something else",
            command: None,
        },
    ];
    let (_, rendered) = render_to_string_with_state(&app, 110, 30);

    let index = rendered
        .help
        .help_entries()
        .position(|entry| entry.label == "Close frame")
        .expect("Help row");

    let (x, y, _, _) = rendered.help.area.expect("help geometry");
    let hit = rendered
        .help
        .help_row_at(x, y + index as u16)
        .expect("row hit");
    assert_eq!(hit, index);

    app.help.index = hit;
    if let Some(command) = app.help.selected_help_command() {
        crate::app::reduce::reduce(&mut app, crate::common::actions::Action::Command(command));
    }
    assert!(app.should_quit, "the clicked row ran its command");
}

#[test]
fn help_row_hit_testing_maps_screen_cells_to_entries() {
    use crate::app::dashboard::render_to_string_with_state;
    let mut app = app();
    app.overlay = Overlay::Help;
    let (_, rendered) = render_to_string_with_state(&app, 110, 30);
    let (x, y, width, height) = rendered.help.area.expect("help geometry recorded");
    assert!(height > 0, "rows were recorded");
    let first = rendered.help.help_row_at(x, y).expect("first row hit");
    assert_eq!(first, rendered.help.offset);
    // Only real entries are hittable: the list box is taller than the entries.
    let entries = rendered.help.help_entries().count() as u16;
    let last = rendered
        .help
        .help_row_at(x + width - 1, y + entries - 1)
        .expect("last entry hit");
    assert_eq!(last, first + entries as usize - 1);
    assert_eq!(
        rendered.help.help_row_at(x, y + entries),
        None,
        "past the last entry there is nothing to hit",
    );
    assert_eq!(rendered.help.help_row_at(0, 0), None, "outside the list");
}

#[test]
fn hovering_a_help_row_moves_the_selection_without_activating_it() {
    let mut app = app();
    app.overlay = Overlay::Help;
    let (_, mut rendered) = crate::app::dashboard::render_to_string_with_state(&app, 110, 30);
    let (x, y, _, _) = rendered.help.area.expect("help geometry");
    assert_eq!(rendered.help.index, 0);

    mouse(
        &mut rendered,
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: x + 3,
            row: y + 1,
            modifiers: KeyModifiers::NONE,
        },
        &crate::app::dashboard::Panels::default(),
    );

    assert_eq!(rendered.help.index, 1, "hover selects the second row");
    assert_eq!(
        rendered.overlay,
        Overlay::Help,
        "hover does not run the row"
    );
}

#[test]
fn help_uses_h_and_appears_once_at_the_bottom_of_the_footer() {
    let mut state = app();
    press(
        &mut state,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    );
    assert_eq!(state.overlay, Overlay::Help);

    state.overlay = Overlay::None;
    let screen = render_to_string(&state, 120, 25);
    assert_eq!(screen.matches("Help").count(), 1, "Help is not duplicated");
    let lines: Vec<_> = screen.lines().collect();
    let help = lines
        .iter()
        .position(|line| line.contains("Help"))
        .expect("Help control");
    assert_eq!(help, lines.len() - 2);
}
