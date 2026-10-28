//! The Settings dialog's tests: the sections its slices contribute, the pointer, the number
//! controls, and what a change does to the file and to the frame.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

use crate::app::dashboard::render_to_string_with_state;
use crate::app::model::Overlay;
use crate::app::testing::{app, press};
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::config::LeaveScreen;

fn open(app: &mut crate::app::model::App) {
    // The dialog floats over the frame the run draws, so the cards behind it stay as they are.
    app.overlay = Overlay::Settings;
}

fn row_of(app: &crate::app::model::App, label: &str) -> usize {
    app.settings
        .entries
        .iter()
        .position(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("no setting named {label}"))
}

#[test]
fn sections_are_separated_by_a_blank_line() {
    use crate::modules::panels::settings::Line;

    let mut app = app();
    open(&mut app);
    let lines = app.settings.lines();
    let activity = lines
        .iter()
        .position(|line| *line == Line::Section("Activity"))
        .expect("the Activity title");
    assert_eq!(
        lines[activity - 1],
        Line::Gap,
        "a blank line sits above every section title but the first"
    );
    assert_eq!(
        lines[0],
        Line::Section("Panels"),
        "the first section starts at the top of the list"
    );

    let (screen, rendered) = render_to_string_with_state(&app, 100, 40);
    let (x, y, width, _) = rendered.settings.area.expect("the list was drawn");
    let rows: Vec<&str> = screen.lines().collect();
    // The dialog's own cells, so the cards behind it cannot answer for the row.
    let row = |index: usize| -> String {
        rows[y as usize + index]
            .chars()
            .skip(x as usize)
            .take(width as usize)
            .collect()
    };
    assert!(
        row(activity).starts_with("  Activity"),
        "the title is drawn: {:?}",
        row(activity)
    );
    assert!(
        row(activity - 1).trim().is_empty(),
        "and the view leaves that row empty"
    );
}

#[test]
fn the_dialog_groups_the_settings_in_sections() {
    let mut app = app();
    open(&mut app);
    let screen = render_to_string_with_state(&app, 100, 40).0;

    for section in ["Panels", "Activity", "Runs", "Cleanup", "Terminal"] {
        assert!(
            screen.contains(section),
            "the `{section}` section is named:\n{screen}"
        );
    }
    assert!(
        screen.contains("'Status' panel at start"),
        "with its settings under it"
    );
    assert!(screen.contains("Logs kept"), "and the recorded runs' own");
    assert!(
        screen.contains("Remove all previous runs") && screen.contains("Cleanup now"),
        "and the buttons its slices offer"
    );
    assert!(
        screen.contains("Recorded runs") && screen.contains("Size on disk"),
        "with the runs' own figures beside the button that cuts them"
    );
    assert!(
        !screen.contains("Raw log lines"),
        "the raw-line toggle is not a card row: the filter dialog keeps it\n{screen}"
    );
}

#[test]
fn the_dialog_is_off_until_its_key_asks_for_it() {
    let mut app = app();
    let before = render_to_string_with_state(&app, 100, 30).0;
    assert!(
        !before.contains("'Logs' panel at start"),
        "no dialog is drawn at start"
    );

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
    );

    assert_eq!(app.overlay, Overlay::Settings, "the key opens the dialog");
    let after = render_to_string_with_state(&app, 100, 30).0;
    assert!(
        after.contains("'Logs' panel at start"),
        "and its rows are on screen:\n{after}"
    );
}

#[test]
fn a_panel_row_sets_the_next_start_without_touching_this_frame() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let mut app = app();
    open(&mut app);
    app.config.show_status = true;
    let before = (app.config.show_status, app.settings.config.show_status);

    app.settings.row = row_of(&app, "'Status' panel at start");
    reduce(
        &mut app,
        Action::Command(crate::modules::panels::settings::Command::ChangeValue(1).into()),
    );

    assert_eq!(
        (app.config.show_status, app.settings.config.show_status),
        (before.0, !before.1),
        "the file gets the new default, the frame keeps what it is showing"
    );
    let screen = render_to_string_with_state(&app, 100, 30).0;
    assert!(
        screen.contains("─ Status"),
        "the Status card is still on screen after its row was turned off"
    );
}

#[test]
fn a_change_edits_the_file_and_asks_for_it_to_be_written() {
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};

    let mut app = app();
    open(&mut app);
    app.settings.row = row_of(&app, "Drop old logs");
    let live = app.config.prune_logs;
    let before = app.settings.config.prune_logs;
    let effects = reduce(
        &mut app,
        Action::Command(crate::modules::panels::settings::Command::ChangeValue(1).into()),
    );

    assert_ne!(
        app.settings.config.prune_logs, before,
        "the row was flipped"
    );
    assert_eq!(
        app.config.prune_logs, live,
        "the running frame is not touched"
    );
    assert!(
        effects.contains(&Effect::SaveSettings),
        "a change is written to the file"
    );
}

#[test]
fn the_runs_rows_change_the_prune_settings() {
    let mut app = app();
    open(&mut app);
    assert!(app.settings.config.prune_logs, "pruning starts on");

    app.settings.row = row_of(&app, "Drop old logs");
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(1),
    );
    assert!(!app.settings.config.prune_logs, "the row turns pruning off");

    app.settings.row = row_of(&app, "Logs kept");
    let before = app.settings.config.keep_logs;
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(1),
    );
    assert_eq!(app.settings.config.keep_logs, before + 1);
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(-1),
    );
    assert_eq!(app.settings.config.keep_logs, before);
}

#[test]
fn a_choice_cycles_and_a_number_steps() {
    let mut app = app();
    open(&mut app);

    app.settings.row = row_of(&app, "Terminal on exit");
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(1),
    );
    assert_eq!(app.settings.config.leave, LeaveScreen::Frame);

    app.settings.row = row_of(&app, "Cleanup after a rebuild");
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(1),
    );
    assert_eq!(app.settings.config.cleanup, CleanupPolicy::Always);

    app.settings.row = row_of(&app, "Generations kept");
    let before = app.settings.config.keep_generations;
    crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(-1),
    );
    assert_eq!(
        app.settings.config.keep_generations,
        before - 1,
        "a step down is honoured"
    );
}

#[test]
fn hovering_selects_the_row_and_a_section_title_selects_nothing() {
    let mut app = app();
    open(&mut app);
    // Seed a selection, or a hover that does nothing would pass unnoticed at row 0.
    app.settings.row = 4;
    let (_, rendered) = render_to_string_with_state(&app, 100, 40);
    let mut app = rendered;
    let (x, y, width, height) = app.settings.area.expect("the list was drawn");
    assert!(height > 2, "the card has rows to point at");

    // The first line is the "Panels" title: pointing at it leaves the selection alone.
    crate::modules::panels::settings::mouse_command(
        &mut app.settings,
        MouseEventKind::Moved,
        x + 2,
        y,
    );
    assert_eq!(app.settings.row, 4, "a title is not a row");

    crate::modules::panels::settings::mouse_command(
        &mut app.settings,
        MouseEventKind::Moved,
        x + width / 2,
        y + 1,
    );
    assert_eq!(app.settings.row, 0, "the setting under the pointer");
}

#[test]
fn a_click_on_a_row_changes_it_and_a_number_control_steps_it() {
    let mut app = app();
    open(&mut app);
    let (_, rendered) = render_to_string_with_state(&app, 100, 40);
    let mut app = rendered;
    let (x, y, width, _) = app.settings.area.expect("the list was drawn");

    // First line is the section, second is "'Status' panel at start".
    let command = crate::modules::panels::settings::mouse_command(
        &mut app.settings,
        MouseEventKind::Down(MouseButton::Left),
        x + width / 2,
        y + 1,
    );
    assert_eq!(
        command,
        Some(crate::modules::panels::settings::Command::ChangeValue(1).into()),
        "a click on a row changes it"
    );

    // A number row draws two controls; the `+` steps up without touching the selection.
    app.settings.row = row_of(&app, "Logs kept");
    let (_, rendered) = render_to_string_with_state(&app, 100, 40);
    let mut app = rendered;
    let (x, y, _, _) = app.settings.area.expect("the list was drawn");
    let line = app.settings.line_of(app.settings.row).expect("the row") as u16;
    let (plus, delta) = app
        .settings
        .buttons
        .iter()
        .find(|(_, delta)| *delta == 1)
        .copied()
        .expect("a `+` control was drawn");
    assert_eq!(
        (plus.0, plus.1),
        (x + 31 + 3 + 2 + 4, y + line),
        "the control sits at the end of its own row, in a column every row shares"
    );
    let command = crate::modules::panels::settings::mouse_command(
        &mut app.settings,
        MouseEventKind::Down(MouseButton::Left),
        plus.0,
        plus.1,
    );
    assert_eq!(
        command,
        Some(crate::modules::panels::settings::Command::ChangeValue(delta).into()),
        "the control reports its own step"
    );
}

#[test]
fn the_selection_never_leaves_the_list() {
    let mut app = app();
    open(&mut app);
    app.settings.move_selection(-99);
    assert_eq!(app.settings.row, 0, "the top is the top");
    app.settings.move_selection(99);
    assert_eq!(
        app.settings.row,
        app.settings.entries.len() - 1,
        "and the bottom is the bottom"
    );
}

#[test]
fn the_list_keeps_the_selected_row_in_view() {
    let mut app = app();
    open(&mut app);
    app.settings.move_selection(99);
    let line = app
        .settings
        .line_of(app.settings.row)
        .expect("the last row");
    app.settings.set_offset(4);

    assert_eq!(
        app.settings.offset,
        line + 1 - 4,
        "the window follows the selection instead of running past it"
    );
}

/// One cell's colour, drawn by the dashboard itself so a test reads what the screen shows.
fn cell_color(app: &crate::app::model::App, column: u16, row: u16) -> ratatui::style::Color {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(100, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut owned = app.clone();
    terminal
        .draw(|frame| {
            crate::app::dashboard::render(frame, &mut owned);
        })
        .expect("render");
    terminal
        .backend()
        .buffer()
        .cell((column, row))
        .expect("the cell was painted")
        .fg
}

#[test]
fn an_off_toggle_reads_red_and_an_on_toggle_reads_green() {
    use crate::common::theme::{GREEN, RED};

    let mut app = app();
    open(&mut app);
    app.settings.config.show_status = false;
    app.settings.config.raw_logs = true;
    let (_, rendered) = render_to_string_with_state(&app, 100, 40);
    let (x, y, _, _) = rendered.settings.area.expect("the list was drawn");
    // The value column starts after the cursor, its padding and the label.
    let value = x + 31;

    let off = rendered.settings.line_of(0).expect("the Run row") as u16;
    assert_eq!(
        cell_color(&app, value, y + off),
        RED,
        "a toggle that is off says so in red"
    );
    let on = rendered
        .settings
        .line_of(row_of(&app, "Drop old logs"))
        .expect("the prune row") as u16;
    assert_eq!(
        cell_color(&app, value, y + on),
        GREEN,
        "and one that is on says so in green"
    );
}

#[test]
fn the_settings_chip_is_lit_like_help() {
    use crate::common::theme::GREEN;

    let (help, settings) = footer_chip_colors(&app());
    assert_eq!(
        settings, GREEN,
        "the settings chip is the green one Help is, closed or open"
    );
    assert_eq!(
        settings, help,
        "the two chips read alike, so neither looks switched off"
    );
}

/// The two chips at the footer's right edge, as the screen draws them.
fn footer_chip_colors(
    app: &crate::app::model::App,
) -> (ratatui::style::Color, ratatui::style::Color) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("test terminal");
    let mut owned = app.clone();
    terminal
        .draw(|frame| {
            crate::app::dashboard::render(frame, &mut owned);
        })
        .expect("render");
    let buffer = terminal.backend().buffer();
    // The footer is the last row of the 39 the cards were given.
    let row = 38;
    let cells: Vec<char> = (0..buffer.area.width)
        .map(|column| {
            buffer
                .cell((column, row))
                .expect("a footer cell")
                .symbol()
                .chars()
                .next()
                .unwrap_or(' ')
        })
        .collect();
    // Cells, never `find`: a Nerd Font glyph is one cell and several bytes.
    let at = |label: &str| {
        let needle: Vec<char> = label.chars().collect();
        cells
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .unwrap_or_else(|| {
                let row: String = cells.iter().collect();
                panic!("no {label} chip on the footer row: {row:?}")
            }) as u16
    };
    // One cell into each label, past the bracket, the glyph and the key.
    (
        cell_color(app, at("Help") + 1, row),
        cell_color(app, at("Settings") + 2, row),
    )
}

#[test]
fn the_dialog_draws_a_bar_only_when_its_list_does_not_fit() {
    let mut app = app();
    open(&mut app);
    let tall = render_to_string_with_state(&app, 100, 40).0;
    assert!(
        !tall.contains('\u{2588}') && !tall.contains('\u{2503}'),
        "no bar while every row fits:\n{tall}"
    );

    let (short, rendered) = render_to_string_with_state(&app, 100, 16);
    assert!(short.contains('\u{2588}'), "a bar once the list overflows");
    let bar = rendered.settings.scrollbar.expect("the bar's own geometry");
    assert_eq!(
        rendered
            .settings
            .lines()
            .len()
            .saturating_sub(rendered.settings.area.expect("the list").3 as usize),
        bar.max_scroll,
        "the bar counts the rows the list cannot show"
    );
}

#[test]
fn the_bar_reads_the_list_from_its_first_line_down() {
    let mut app = app();
    open(&mut app);
    let (_, rendered) = render_to_string_with_state(&app, 100, 16);
    let bar = rendered.settings.scrollbar.expect("the card overflows");

    assert_eq!(
        bar.top_offset_at(bar.y, 0),
        0,
        "the top of the track is the first line"
    );
    let bottom = bar.top_offset_at(bar.y + bar.height - 1, 0);
    assert_eq!(bottom, bar.max_scroll, "and the bottom is the last one");

    let mut state = rendered.settings.clone();
    crate::modules::panels::settings::reduce(
        &mut state,
        crate::modules::panels::settings::Command::SetScroll(bottom),
    );
    assert!(state.offset > 0, "a drag to the bottom moves the list");
    assert!(
        state.offset_pinned,
        "the pointer's place is kept until a key moves the selection"
    );
    state.move_selection(1);
    assert!(!state.offset_pinned, "a key takes the view back");
}

#[test]
fn a_button_row_runs_its_command_and_writes_no_value() {
    use crate::modules::panels::settings::{Command, Effect};

    let mut app = app();
    open(&mut app);
    let before = app.settings.config.clone();
    app.settings.row = row_of(&app, "Cleanup now");

    let effects =
        crate::modules::panels::settings::reduce(&mut app.settings, Command::ChangeValue(1));

    assert_eq!(
        effects,
        vec![Effect::Run(
            crate::modules::work::cleanup::Command::AskCleanup.into()
        )],
        "the row is a button, so it names the command its own slice would send"
    );
    assert_eq!(
        app.settings.config, before,
        "and there is no value to write back"
    );
}

#[test]
fn the_cleanup_button_asks_the_same_question_a_finished_rebuild_asks() {
    use crate::app::model::Overlay;
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};

    let mut app = app();
    open(&mut app);
    app.settings.row = row_of(&app, "Cleanup now");

    let effects = reduce(
        &mut app,
        Action::Command(crate::modules::panels::settings::Command::ChangeValue(1).into()),
    );

    assert_eq!(
        effects,
        vec![Effect::None],
        "the button only asks; the pass waits for the answer"
    );
    assert_eq!(
        app.overlay,
        Overlay::CleanupPrompt,
        "and the question is the one a finished rebuild opens"
    );
}

#[test]
fn the_cleanup_question_returns_to_the_dialog_that_asked_it() {
    use crate::app::reduce::reduce;
    use crate::common::actions::Action;

    let mut app = app();
    open(&mut app);
    app.settings.row = row_of(&app, "Cleanup now");
    reduce(
        &mut app,
        Action::Command(crate::modules::panels::settings::Command::ChangeValue(1).into()),
    );
    assert_eq!(app.overlay, Overlay::CleanupPrompt, "the button asks first");

    // `n` is the answer a finished rebuild's question takes for no.
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
    );

    assert_eq!(
        app.overlay,
        Overlay::Settings,
        "declining puts the settings back in front, as the card behind the question did"
    );
}

#[test]
fn the_cleanup_button_waits_while_a_rebuild_is_building() {
    use crate::app::model::Overlay;
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};
    use crate::common::phase::Phase;

    let mut app = app();
    open(&mut app);
    app.status.phase = Phase::Building;
    app.settings.row = row_of(&app, "Cleanup now");

    let effects = reduce(
        &mut app,
        Action::Command(crate::modules::panels::settings::Command::ChangeValue(1).into()),
    );

    assert_eq!(
        effects,
        vec![Effect::None],
        "a pass under a build would take the store from it"
    );
    assert_eq!(
        app.status.phase,
        Phase::Building,
        "so the run is left alone"
    );
    assert_eq!(
        app.overlay,
        Overlay::Settings,
        "and no question is asked while it is still building"
    );
}

#[test]
fn the_remove_all_previous_runs_row_asks_before_the_runs_are_cut() {
    let mut app = app();
    open(&mut app);
    app.settings.row = row_of(&app, "Remove all previous runs");

    let effects = crate::modules::panels::settings::reduce(
        &mut app.settings,
        crate::modules::panels::settings::Command::ChangeValue(1),
    );

    assert_eq!(
        effects,
        vec![crate::modules::panels::settings::Effect::Run(
            crate::modules::runs::records::Command::RemovePrevious.into()
        )],
        "the button names the command that asks the question"
    );
    // And that command asks rather than deletes, so a stray Enter cannot empty the records.
    use crate::app::reduce::reduce;
    use crate::common::actions::{Action, Effect};
    assert_eq!(
        reduce(
            &mut app,
            Action::Command(crate::modules::runs::records::Command::RemovePrevious.into())
        ),
        vec![Effect::AskRemoveRuns],
        "the answer is wanted before anything is deleted"
    );
}

#[test]
fn a_read_only_row_is_stepped_over_by_the_arrows() {
    let mut app = app();
    open(&mut app);
    let runs = row_of(&app, "Recorded runs");
    let size = row_of(&app, "Size on disk");
    // The row the arrows start from: the last one of the section above the figures.
    let before = runs - 1;
    app.settings.row = before;

    app.settings.move_selection(1);
    assert_eq!(
        app.settings.row,
        runs + 2,
        "the two figures under the toggle are passed over"
    );
    app.settings.move_selection(-1);
    assert_eq!(app.settings.row, before, "and back the other way");
    assert_ne!(size, app.settings.row);
}

#[test]
fn scrolling_back_up_reaches_the_first_section_title() {
    let mut app = app();
    open(&mut app);
    app.settings.move_selection(99);
    let (_, rendered) = render_to_string_with_state(&app, 100, 16);
    let viewport = rendered.settings.area.expect("the list").3 as usize;
    let mut state = rendered.settings.clone();
    assert!(state.offset > 0, "the list was scrolled down first");

    state.move_selection(-99);
    state.set_offset(viewport);

    assert_eq!(
        state.offset, 0,
        "the top of the list is the top of the view"
    );
    let first = state.lines().first().copied().expect("the first line");
    assert_eq!(
        first,
        crate::modules::panels::settings::Line::Section("Panels"),
        "so the title the settings hang from is on screen again"
    );
}
