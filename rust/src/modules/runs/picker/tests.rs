//! Tests for the selector: mark movement, search, column order, and pointer routing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::testing::run_meta;

#[test]
fn the_selector_moves_within_the_list_and_opens_what_is_under_the_mark() {
    use super::{PickerStep, Selector};
    use crossterm::event::KeyEventKind;

    let mut selector = Selector::new(vec![
        run_meta("20261013-221434-258d1", "home", Some(0), "nothing to do"),
        run_meta(
            "20261012-120000-abcde",
            "system",
            Some(1),
            "nixos-rebuild switch exited with 1",
        ),
        run_meta("20261011-090000-fffff", "home", None, "running"),
    ]);
    let press =
        |selector: &mut Selector, code| selector.press(KeyEvent::new(code, KeyModifiers::NONE));

    // The mark moves one row at a time and stops at the ends instead of wrapping.
    assert_eq!(selector.selected(), 0);
    assert_eq!(press(&mut selector, KeyCode::Down), PickerStep::Continue);
    assert_eq!(selector.selected(), 1);
    assert_eq!(
        press(&mut selector, KeyCode::Char('j')),
        PickerStep::Continue
    );
    assert_eq!(selector.selected(), 2);
    assert_eq!(press(&mut selector, KeyCode::Down), PickerStep::Continue);
    assert_eq!(selector.selected(), 2, "it stops at the last run");
    assert_eq!(press(&mut selector, KeyCode::Up), PickerStep::Continue);
    assert_eq!(selector.selected(), 1);
    assert_eq!(press(&mut selector, KeyCode::Home), PickerStep::Continue);
    assert_eq!(selector.selected(), 0);
    assert_eq!(press(&mut selector, KeyCode::End), PickerStep::Continue);
    assert_eq!(selector.selected(), 2);

    // Enter opens the run under the mark, by id.
    assert_eq!(
        press(&mut selector, KeyCode::Enter),
        PickerStep::Open("20261011-090000-fffff".into())
    );

    // Esc and q leave, and so does Ctrl+C, which raw mode delivers as a key.
    assert_eq!(press(&mut selector, KeyCode::Esc), PickerStep::Leave);
    assert_eq!(press(&mut selector, KeyCode::Char('q')), PickerStep::Leave);
    assert_eq!(
        selector.press(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        PickerStep::Leave
    );

    // Crossterm reports a release as well as a press; only the press counts.
    let mut release = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(selector.press(release), PickerStep::Continue);
    assert_eq!(selector.selected(), 2, "a release does not move the mark");
}

#[test]
fn the_selector_searches_the_runs_it_shows() {
    use super::{PickerStep, Selector};

    let mut selector = Selector::new(vec![
        run_meta(
            "20261013-221434-258d1",
            "home",
            Some(0),
            "nothing to do — every path was already in the store",
        ),
        run_meta(
            "20261012-120000-abcde",
            "system",
            Some(1),
            "nixos-rebuild switch exited with 1",
        ),
        run_meta(
            "20261011-090000-fffff",
            "home",
            Some(0),
            "400 activities completed",
        ),
    ]);
    let press =
        |selector: &mut Selector, code| selector.press(KeyEvent::new(code, KeyModifiers::NONE));
    let type_in = |selector: &mut Selector, text: &str| {
        for letter in text.chars() {
            selector.press(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::NONE));
        }
    };

    // While the search is open the letters are text rather than keys.
    assert_eq!(
        press(&mut selector, KeyCode::Char('/')),
        PickerStep::Continue
    );
    assert!(selector.filtering());
    type_in(&mut selector, "system");
    assert_eq!(selector.query(), "system");
    assert_eq!(selector.visible().len(), 1, "only the run that says so");
    assert_eq!(selector.visible()[0].id, "20261012-120000-abcde");
    assert_eq!(selector.selected(), 0, "the mark lands on what is left");

    assert_eq!(
        press(&mut selector, KeyCode::Enter),
        PickerStep::Open("20261012-120000-abcde".into())
    );

    assert_eq!(
        press(&mut selector, KeyCode::Backspace),
        PickerStep::Continue
    );
    assert_eq!(selector.query(), "syste");
    assert_eq!(selector.visible().len(), 1);

    // The search reads the run's status too, which the list does not show.
    assert_eq!(press(&mut selector, KeyCode::Esc), PickerStep::Continue);
    assert!(!selector.filtering());
    assert_eq!(selector.query(), "");
    assert_eq!(selector.visible().len(), 3);
    assert_eq!(
        press(&mut selector, KeyCode::Char('/')),
        PickerStep::Continue
    );
    type_in(&mut selector, "nothing to do");
    assert_eq!(
        selector.visible().len(),
        1,
        "the status is searched as well"
    );
    assert_eq!(selector.visible()[0].id, "20261013-221434-258d1");

    // Enter with nothing matched closes the selector instead of opening a run.
    type_in(&mut selector, "zzz");
    assert!(selector.visible().is_empty());
    assert_eq!(press(&mut selector, KeyCode::Enter), PickerStep::Leave);
}

#[test]
fn the_selector_lists_the_recorded_runs_and_marks_the_chosen_one() {
    let runs = [
        run_meta(
            "20261013-214512-1a2b",
            "system",
            Some(0),
            "system switch completed",
        ),
        run_meta(
            "20261012-090000-3c4d",
            "home",
            Some(1),
            "home manager switch exited with 1",
        ),
    ];

    let rows: Vec<&crate::common::records::Meta> = runs.iter().collect();
    let (screen, geometry) = super::picker_to_string(&rows, 1, "", false, None, 120, 12);
    assert!(
        screen.contains("Recorded runs"),
        "the frame names itself:\n{screen}"
    );
    assert!(
        screen.contains("[↑↓  Choose] [Enter  Open] [/  Search] [Esc  Leave]"),
        "the keys it answers to are on screen:\n{screen}"
    );
    assert!(screen.contains("WHEN"), "the columns are headed:\n{screen}");
    assert!(screen.contains("DURATION"), "got:\n{screen}");
    assert!(screen.contains("CHECKOUT"), "got:\n{screen}");
    assert!(screen.contains("20261013-214512-1a2b"));
    assert!(screen.contains("20261012-090000-3c4d"));
    assert!(
        screen.contains("/srv/checkouts/nixos-dotfiles#sample"),
        "the checkout is a column of its own:\n{screen}"
    );
    assert!(
        screen.contains("exit 1"),
        "how a run ended is a column of its own:\n{screen}"
    );
    assert!(
        !screen.contains("system switch completed"),
        "the status sentence is left to the run's own frame:\n{screen}"
    );

    // Row order: the frame border, a blank row, the headings, then the runs.
    let lines: Vec<&str> = screen.lines().collect();
    let border = lines[0];
    assert!(border.starts_with('╭'), "the frame is the first row");
    assert!(
        !lines[1].chars().any(|cell| cell.is_alphanumeric()),
        "a blank row under the frame:\n{screen}"
    );
    assert!(
        lines[2].contains("WHEN"),
        "the headings come next:\n{screen}"
    );
    assert!(
        lines[3].contains("20261013-214512-1a2b"),
        "and then the first run:\n{screen}"
    );

    // A row, a heading and a chip must be reachable in the geometry the pointer is matched against.
    assert_eq!(geometry.title_row, 0);
    assert_eq!(geometry.header_row, 2);
    assert_eq!(geometry.first_row, 3);
    assert_eq!(geometry.row_at(3), Some(0));
    assert_eq!(geometry.row_at(4), Some(1));
    assert_eq!(geometry.row_at(2), None, "the heading row is not a run");
    assert_eq!(geometry.row_at(5), None, "nor is the blank space below");
    assert_eq!(
        geometry.column_at(geometry.columns[0].0, 2),
        Some(crate::common::records::Column::When),
        "the first column's own range names it"
    );
    assert_eq!(geometry.column_at(3, 3), None, "only the heading row sorts");

    // While the search is open the title carries the query and the keys change.
    let (searching, searching_geometry) =
        super::picker_to_string(&rows, 0, "home", true, None, 120, 12);
    assert!(
        searching.contains("Recorded runs  /home"),
        "the search is on screen:\n{searching}"
    );
    assert!(
        searching.contains("[Enter  Open] [Esc  Clear]"),
        "so are its keys:\n{searching}"
    );
    assert!(
        searching_geometry.chip_at(100, 0).is_some(),
        "a chip is where the title drew it"
    );
}

#[test]
fn the_selector_scrolls_to_keep_the_chosen_run_in_view() {
    let runs: Vec<crate::common::records::Meta> = (0..20)
        .map(|index| {
            run_meta(
                &format!("202610{:02}-120000-abc{index}", index + 1),
                "home",
                Some(0),
                "home manager switch completed",
            )
        })
        .collect();
    let rows: Vec<&crate::common::records::Meta> = runs.iter().collect();

    // A frame too short for the list scrolls the first run off and keeps the chosen one.
    let (screen, _) = super::picker_to_string(&rows, rows.len() - 1, "", false, None, 120, 8);
    assert!(
        screen.contains("20261020-120000-abc19"),
        "the chosen run stays in view:\n{screen}"
    );
    assert!(
        !screen.contains("20261001-120000-abc0 "),
        "the list scrolled past the top:\n{screen}"
    );

    let (top, _) = super::picker_to_string(&rows[..2], 0, "", false, None, 120, 8);
    assert!(top.contains("20261001-120000-abc0"));
}

#[test]
fn clicking_a_heading_orders_the_list_by_that_column_and_round_trips() {
    use super::Selector;
    use crate::common::records::{Column, Order};

    let runs = vec![
        run_meta("20261013-221434-258d1", "home", Some(0), "ok"),
        run_meta(
            "20261012-120000-abcde",
            "system",
            Some(1),
            "nixos-rebuild switch exited with 1",
        ),
        run_meta("20261011-090000-fffff", "home", Some(0), "ok"),
    ];
    // The fixture gives every run the same start time; ordering by date needs dates that differ.
    let mut runs = runs;
    for (index, meta) in runs.iter_mut().enumerate() {
        meta.started_at -= index as u64 * 86_400;
    }
    let ids = |selector: &Selector| {
        selector
            .visible()
            .iter()
            .map(|meta| meta.id.clone())
            .collect::<Vec<_>>()
    };
    let mut selector = Selector::new(runs);

    // Without an arrow the list stays in the order the runs are kept in: newest first.
    assert_eq!(selector.sort(), None);
    assert_eq!(
        ids(&selector),
        [
            "20261013-221434-258d1",
            "20261012-120000-abcde",
            "20261011-090000-fffff"
        ]
    );

    // Clicks cycle: ascending, then descending, then back to the kept order.
    selector.sort_by(Column::Id);
    assert_eq!(selector.sort(), Some((Column::Id, Order::Ascending)));
    assert_eq!(
        ids(&selector),
        [
            "20261011-090000-fffff",
            "20261012-120000-abcde",
            "20261013-221434-258d1"
        ]
    );
    selector.sort_by(Column::Id);
    assert_eq!(selector.sort(), Some((Column::Id, Order::Descending)));
    assert_eq!(
        ids(&selector),
        [
            "20261013-221434-258d1",
            "20261012-120000-abcde",
            "20261011-090000-fffff"
        ]
    );
    selector.sort_by(Column::Id);
    assert_eq!(
        selector.sort(),
        None,
        "a third click is the list as it is kept"
    );

    // When orders as a time, not as the shown text, which would put `07 Oct` after `01 Nov`.
    selector.sort_by(Column::When);
    assert_eq!(ids(&selector)[0], "20261011-090000-fffff");
    selector.sort_by(Column::When);
    assert_eq!(ids(&selector)[0], "20261013-221434-258d1");

    // The other columns order by what they show, so `exit 1` comes before `ok`.
    selector.sort_by(Column::Result);
    assert_eq!(selector.sort(), Some((Column::Result, Order::Ascending)));
    assert_eq!(ids(&selector)[0], "20261012-120000-abcde");
    selector.sort_by(Column::Scope);
    assert_eq!(selector.sort(), Some((Column::Scope, Order::Ascending)));
    assert_eq!(ids(&selector)[0], "20261013-221434-258d1", "home first");

    // Changing the order puts the mark back at the top.
    selector.mark_row(2);
    selector.sort_by(Column::Duration);
    assert_eq!(selector.selected(), 0);

    // The ordered column carries the arrow and the run row stays lined up under it.
    let rows: Vec<&crate::common::records::Meta> = selector.visible();
    let (screen, _) = super::picker_to_string(
        &rows,
        0,
        "",
        false,
        Some((Column::Result, Order::Descending)),
        120,
        12,
    );
    assert!(
        screen.contains("RESULT ↓"),
        "the ordered column says so:\n{screen}"
    );
    let headings = screen
        .lines()
        .find(|line| line.contains("RESULT"))
        .expect("the heading row");
    let row = screen
        .lines()
        .find(|line| line.contains("2026101"))
        .expect("a run row");
    // Every heading must start over its own column's values even with an arrow.
    // Offsets are in cells, not bytes: a glyph like the row marker is one cell and three bytes.
    let at = |line: &str, needle: &str| {
        let line: Vec<char> = line.chars().collect();
        let needle: Vec<char> = needle.chars().collect();
        line.windows(needle.len())
            .position(|window| window == needle.as_slice())
            .unwrap_or_else(|| panic!("`{needle:?}` is not in the row:\n{screen}"))
    };
    // The date is read from the row itself: the column shows the local day, which
    // moves with the machine's offset, and only its width matters here.
    let when = rows[0].when();
    let when_day: String = when.chars().take(6).collect();
    for (heading, value) in [
        ("WHEN", when_day.as_str()),
        ("ID", "2026101"),
        ("SCOPE", "home"),
        ("RESULT", "ok"),
        ("DURATION", "01:00"),
        ("CHECKOUT", "/srv/checkouts"),
    ] {
        assert_eq!(
            at(headings, heading),
            at(row, value),
            "`{heading}` is not lined up over its values:\n{screen}"
        );
    }
}

#[test]
fn a_click_lands_on_the_row_and_the_heading_the_frame_drew() {
    use super::{PickerStep, Selector, picker_mouse};
    use crate::common::records::{Column, Order};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let runs = vec![
        run_meta("20261013-221434-258d1", "home", Some(0), "ok"),
        run_meta(
            "20261012-120000-abcde",
            "system",
            Some(1),
            "nixos-rebuild switch exited with 1",
        ),
        run_meta("20261011-090000-fffff", "home", Some(0), "ok"),
    ];
    let rows: Vec<&crate::common::records::Meta> = runs.iter().collect();
    let (screen, geometry) = super::picker_to_string(&rows, 0, "", false, None, 120, 12);
    let mut selector = Selector::new(runs);
    let click = |column: u16, row: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let lines: Vec<&str> = screen.lines().collect();
    // Pointer cells are the frame's own cells with no conversion; converting them would open the row above.
    let at = |needle: &str| {
        lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("`{needle}` is not on screen:\n{screen}"))
    };
    let column_of = |row: usize, needle: &str| {
        let chars: Vec<char> = lines[row].chars().collect();
        let wanted: Vec<char> = needle.chars().collect();
        chars
            .windows(wanted.len())
            .position(|window| window == wanted.as_slice())
            .unwrap_or_else(|| panic!("`{needle}` is not on row {row}:\n{screen}"))
    };

    let run_row = at("20261012-120000-abcde");
    assert_eq!(
        picker_mouse(&mut selector, &geometry, click(10, run_row as u16)),
        PickerStep::Open("20261012-120000-abcde".into())
    );

    let heading_row = at("SCOPE");
    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(column_of(heading_row, "SCOPE") as u16, heading_row as u16)
        ),
        PickerStep::Continue
    );
    assert_eq!(selector.sort(), Some((Column::Scope, Order::Ascending)));

    // Chips live on the title row, and the leave chip closes the selector.
    let title_row = at("Recorded runs");
    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(
                (column_of(title_row, "[Esc  Leave]") + 6) as u16,
                title_row as u16
            )
        ),
        PickerStep::Leave
    );
}

#[test]
fn the_pointer_does_what_the_keys_do_in_the_selector() {
    use super::PickerChip;
    use super::{PickerStep, Selector, picker_mouse};
    use crate::common::records::{Column, Order};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let runs = vec![
        run_meta("20261013-221434-258d1", "home", Some(0), "ok"),
        run_meta(
            "20261012-120000-abcde",
            "system",
            Some(1),
            "nixos-rebuild switch exited with 1",
        ),
        run_meta("20261011-090000-fffff", "home", Some(0), "ok"),
    ];
    let rows: Vec<&crate::common::records::Meta> = runs.iter().collect();
    let (_, geometry) = super::picker_to_string(&rows, 0, "", false, None, 120, 12);
    // The search frame draws other chips, so the pointer matches the frame on screen.
    let (_, searching) = super::picker_to_string(&rows, 0, "", true, None, 120, 12);
    let mut selector = Selector::new(runs);

    let at = |kind: MouseEventKind, column: u16, row: u16| MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let hover = |column: u16, row: u16| at(MouseEventKind::Moved, column, row);
    let click = |column: u16, row: u16| at(MouseEventKind::Down(MouseButton::Left), column, row);

    assert_eq!(
        picker_mouse(&mut selector, &geometry, hover(10, geometry.first_row + 2)),
        PickerStep::Continue
    );
    assert_eq!(selector.selected(), 2);

    // Hovering the heading row leaves the mark alone: it is not a run.
    picker_mouse(&mut selector, &geometry, hover(10, geometry.header_row));
    assert_eq!(selector.selected(), 2);

    assert_eq!(
        picker_mouse(&mut selector, &geometry, click(10, geometry.first_row + 1)),
        PickerStep::Open("20261012-120000-abcde".into())
    );

    let (when_start, _) = geometry.columns[0];
    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(when_start + 1, geometry.header_row)
        ),
        PickerStep::Continue
    );
    assert_eq!(selector.sort(), Some((Column::When, Order::Ascending)));

    // A click on a chip presses the key it names and never reaches the list.
    let chip = |geometry: &super::PickerGeometry, wanted: PickerChip| {
        geometry
            .chips
            .iter()
            .find(|(chip, _, _)| *chip == wanted)
            .map(|(_, start, end)| (*start + *end) / 2)
            .expect("the chip is on the title row")
    };
    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(chip(&geometry, PickerChip::Search), geometry.title_row)
        ),
        PickerStep::Continue
    );
    assert!(selector.filtering(), "the search chip opens the search");
    assert_eq!(
        picker_mouse(
            &mut selector,
            &searching,
            click(chip(&searching, PickerChip::Clear), searching.title_row)
        ),
        PickerStep::Continue
    );
    assert!(!selector.filtering(), "and the clear chip drops it again");

    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(chip(&geometry, PickerChip::Leave), geometry.title_row)
        ),
        PickerStep::Leave
    );

    selector.mark_row(0);
    picker_mouse(
        &mut selector,
        &geometry,
        at(MouseEventKind::ScrollDown, 10, geometry.first_row),
    );
    assert_eq!(
        selector.selected(),
        2,
        "three rows down, stopped at the end"
    );
    picker_mouse(
        &mut selector,
        &geometry,
        at(MouseEventKind::ScrollUp, 10, geometry.first_row),
    );
    assert_eq!(selector.selected(), 0);

    assert_eq!(
        picker_mouse(&mut selector, &geometry, click(10, geometry.first_row + 3)),
        PickerStep::Continue
    );
    assert_eq!(selector.selected(), 0);
}
