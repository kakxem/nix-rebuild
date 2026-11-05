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
    let (screen, geometry) = super::picker_to_string(&rows, 1, "", false, None, 130, 12);
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

    // Row order: the state card, then the list card's border, its air, its headings, then the runs.
    let lines: Vec<&str> = screen.lines().collect();
    assert!(
        lines[0].starts_with("╭─ Now "),
        "the state card is the first row:\n{screen}"
    );
    assert!(
        lines[3].starts_with("╭─ Recorded runs "),
        "the list is a card of its own under it:\n{screen}"
    );
    assert!(
        lines[4].chars().all(|c| matches!(c, '│' | ' ')),
        "one row of air separates the border from the headings:\n{screen}"
    );
    assert!(
        lines[5].contains("WHEN"),
        "the headings come next:\n{screen}"
    );
    assert!(
        lines[6].contains("20261013-214512-1a2b"),
        "and then the first run:\n{screen}"
    );

    // A row, a heading and a chip must be reachable in the geometry the pointer is matched against.
    assert_eq!(geometry.title_row, 3, "the legend rides the list's border");
    assert_eq!(geometry.header_row, 5);
    assert_eq!(geometry.first_row, 6);
    assert_eq!(geometry.row_at(6), Some(0));
    assert_eq!(geometry.row_at(7), Some(1));
    assert_eq!(geometry.row_at(5), None, "the heading row is not a run");
    assert_eq!(geometry.row_at(8), None, "nor is the blank space below");
    assert_eq!(
        geometry.column_at(geometry.columns[0].0, 5),
        Some(crate::common::records::Column::When),
        "the first column's own range names it"
    );
    assert_eq!(geometry.column_at(3, 5), None, "only the heading row sorts");

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
        searching_geometry.chip_at(100, 3).is_some(),
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
    let (screen, _) = super::picker_to_string(&rows, rows.len() - 1, "", false, None, 120, 10);
    assert!(
        screen.contains("20261020-120000-abc19"),
        "the chosen run stays in view:\n{screen}"
    );
    assert!(
        !screen.contains("20261001-120000-abc0 "),
        "the list scrolled past the top:\n{screen}"
    );

    let (top, _) = super::picker_to_string(&rows[..2], 0, "", false, None, 120, 10);
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

#[test]
fn only_a_run_that_kept_a_lock_asks_before_it_is_put_back() {
    use super::{PickerStep, Selector};

    let mut kept = run_meta("20261029-131900-aaaaa", "system", Some(0), "ok");
    kept.kept = Some("20261029-131845-bbbb1".into());
    let mut plain = run_meta("20261028-090000-ccccc", "home", Some(0), "ok");
    plain.kept = None;
    let mut selector = Selector::new(vec![kept, plain]);
    let press =
        |selector: &mut Selector, code| selector.press(KeyEvent::new(code, KeyModifiers::NONE));

    // A run that kept nothing has nothing to put back, so the key is not an answer there.
    press(&mut selector, KeyCode::Down);
    assert!(!selector.can_roll_back());
    assert_eq!(
        press(&mut selector, KeyCode::Char('r')),
        PickerStep::Continue
    );
    assert!(
        selector.confirming().is_none(),
        "no question without a lock"
    );

    // The one that kept a lock asks first, and Esc keeps the current lock.
    press(&mut selector, KeyCode::Up);
    assert!(selector.can_roll_back());
    assert_eq!(
        press(&mut selector, KeyCode::Char('R')),
        PickerStep::Continue
    );
    assert_eq!(
        selector.confirming().map(|meta| meta.id.as_str()),
        Some("20261029-131900-aaaaa"),
        "the question names the run under the mark"
    );
    assert_eq!(press(&mut selector, KeyCode::Esc), PickerStep::Continue);
    assert!(selector.confirming().is_none(), "Esc says no");

    // While the question is up the list does not move under it.
    press(&mut selector, KeyCode::Char('r'));
    assert_eq!(press(&mut selector, KeyCode::Down), PickerStep::Continue);
    assert_eq!(selector.selected(), 0, "the question owns the keys");
    assert_eq!(
        press(&mut selector, KeyCode::Char('n')),
        PickerStep::Continue
    );
    assert!(selector.confirming().is_none(), "n says no too");

    // `y` confirms and hands back the lock that run kept.
    press(&mut selector, KeyCode::Char('r'));
    assert_eq!(
        press(&mut selector, KeyCode::Char('y')),
        PickerStep::Rollback("20261029-131845-bbbb1".into())
    );
    assert!(selector.confirming().is_none(), "the question is put down");

    // Enter answers it as yes, the same way it opens the marked run otherwise.
    press(&mut selector, KeyCode::Char('r'));
    assert_eq!(
        press(&mut selector, KeyCode::Enter),
        PickerStep::Rollback("20261029-131845-bbbb1".into())
    );
}

#[test]
fn the_roll_back_key_rides_the_title_and_falls_back_to_the_footer() {
    use super::{PickerChip, picker_to_confirm_geometry, picker_to_string};

    let mut kept = run_meta("20261029-131900-aaaaa", "system", Some(0), "ok");
    kept.kept = Some("20261029-131845-bbbb1".into());
    let plain = run_meta("20261028-090000-ccccc", "home", Some(0), "ok");

    // A wide title carries `R` with the other keys, and the footer keeps its blank row.
    let (wide, geometry) = picker_to_string(&[&kept], 0, "", false, None, 100, 12);
    assert!(wide.contains("[R  Revert]"), "got\n{wide}");
    let chip = geometry
        .chips
        .iter()
        .find(|(chip, _, _)| *chip == PickerChip::Rollback)
        .expect("the title carries R");
    assert_eq!(
        geometry.chip_at(chip.1 + 1, geometry.title_row),
        Some(PickerChip::Rollback)
    );
    assert!(
        geometry.rollback.is_none(),
        "the footer has nothing left to say"
    );
    assert!(!wide.contains("Revert this run's lock"), "got\n{wide}");

    // A narrow one cannot hold five chips, so the footer carries the key instead.
    let (narrow, geometry) = picker_to_string(&[&kept], 0, "", false, None, 80, 12);
    assert!(!narrow.contains("[R  Revert]"), "got\n{narrow}");
    let (row, start, _) = geometry.rollback.expect("the hint was drawn");
    assert_eq!(geometry.chip_at(start + 1, row), Some(PickerChip::Rollback));
    assert!(narrow.contains("Revert this run's lock"), "got\n{narrow}");

    // A run that kept no lock has no key at all.
    let (without, geometry) = picker_to_string(&[&plain], 0, "", false, None, 100, 12);
    assert!(!without.contains("Revert"), "got\n{without}");
    assert!(
        geometry.rollback.is_none(),
        "no hint where the key would do nothing"
    );

    // The question names the move: the lock the run built with leaves, the kept one arrives.
    let mut built = kept.clone();
    built.lock = "7d67a5a8c0ffee00".into();
    let (question, _) = picker_to_confirm_geometry(&[&built], 0, Some("9382a635aaaaaaaa"), 100, 20);
    assert!(
        question.contains("Revert the lock the run of"),
        "got\n{question}"
    );
    assert!(
        question.contains("7d67a5a8 → 9382a635"),
        "the box names both ends of the move\ngot\n{question}"
    );
    assert!(
        question.contains("[Y  Revert and rebuild]"),
        "got\n{question}"
    );
    assert!(
        !question.contains("can be undone"),
        "the box says the question, not what the store keeps\ngot\n{question}"
    );
    assert!(
        question.contains("╭ Rollback "),
        "the same title the locks selector uses\ngot\n{question}"
    );
    assert!(
        !question.contains("You land on the state"),
        "the box says only what the row cannot\ngot\n{question}"
    );

    // A run with no digest of its own that kept a lock is still one move, and says so.
    let (arrow, _) = picker_to_confirm_geometry(&[&kept], 0, Some("9382a635aaaaaaaa"), 100, 20);
    assert!(arrow.contains("→ 9382a635"), "got\n{arrow}");

    // The list stays readable around the box: a row above it still shows its id.
    let (question, _) = picker_to_confirm_geometry(&[&kept, &plain], 0, None, 100, 26);
    assert!(
        question.contains("20261029-131900-aaaaa"),
        "got\n{question}"
    );
}

#[test]
fn the_roll_back_chips_take_a_click_like_a_key() {
    use super::{PickerStep, Selector, picker_mouse, picker_to_confirm_geometry};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut kept = run_meta("20261029-131900-aaaaa", "system", Some(0), "ok");
    kept.kept = Some("20261029-131845-bbbb1".into());
    let (_, geometry) = picker_to_confirm_geometry(&[&kept], 0, None, 100, 20);
    let answers = geometry.confirm.expect("the question drew its answers");
    let click = |column: u16, row: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    let mut selector = Selector::new(vec![kept]);
    let ask = |selector: &mut Selector| {
        assert_eq!(
            selector.press(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            PickerStep::Continue
        );
        assert!(selector.confirming().is_some(), "R asks first");
    };

    // `[N]` puts the question down, and `[Y]` hands back the lock the run kept.
    ask(&mut selector);
    assert_eq!(
        picker_mouse(&mut selector, &geometry, click(answers.no.0, answers.no.1)),
        PickerStep::Continue
    );
    assert!(selector.confirming().is_none(), "the chip says no");
    ask(&mut selector);
    assert_eq!(
        picker_mouse(
            &mut selector,
            &geometry,
            click(answers.yes.0, answers.yes.1)
        ),
        PickerStep::Rollback("20261029-131845-bbbb1".into())
    );

    // The list behind the box takes no click while the question is up.
    ask(&mut selector);
    assert_eq!(
        picker_mouse(&mut selector, &geometry, click(2, 4)),
        PickerStep::Continue
    );
    assert!(
        selector.confirming().is_some(),
        "a stray click answers nothing"
    );
}

#[test]
fn the_title_keeps_its_words_on_a_narrow_terminal() {
    use super::picker_to_string;

    let kept = run_meta("20261029-131900-aaaaa", "system", Some(0), "ok");
    let (screen, _) = picker_to_string(&[&kept], 0, "", false, None, 80, 10);
    // The legend is right-aligned over the title: five chips would run into the words.
    assert!(screen.contains("Recorded runs ─"), "got\n{screen}");
    assert!(screen.contains("[Esc  Leave]"), "got\n{screen}");
}

#[test]
fn the_run_that_built_what_the_checkout_holds_is_marked() {
    use super::view::picker_to_string_now;

    let mut built = run_meta("20261031-215800-4bc64", "system", Some(0), "ok");
    built.lock = "64c10c1daaaaaaaa".into();
    let mut older = run_meta("20261029-124000-4bc29", "system", Some(0), "ok");
    older.lock = "9382a635bbbbbbbb".into();
    let runs: Vec<&crate::common::records::Meta> = vec![&built, &older];

    // The digest the checkout holds now names one row, and the card above the list names it too.
    // The mark sits on the other row, so the bullet has to be what marks the current one.
    let screen = picker_to_string_now(&runs, 1, &built.lock, 130, 16);
    let rows: Vec<&str> = screen.lines().collect();
    assert!(
        screen.contains("64c10c1d  ·"),
        "the state card names the digest\ngot\n{screen}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains('\u{2022}') && row.contains("20261031-215800-4bc64")),
        "the run that built it is marked\ngot\n{screen}"
    );
    assert!(
        rows.iter().filter(|row| row.contains('\u{2022}')).count() == 1,
        "and only that one\ngot\n{screen}"
    );

    // A checkout that has moved past every recorded run marks nothing and still says where it is.
    let screen = picker_to_string_now(&runs, 1, "ffffffffffffffff", 130, 16);
    assert!(screen.contains("ffffffff"), "got\n{screen}");
    assert!(
        !screen.contains('\u{2022}'),
        "no row is marked\ngot\n{screen}"
    );
}
