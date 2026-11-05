//! The saved locks: what a save keeps, what a restore puts back, the selector's keys, and the
//! Settings row that bounds the store.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::picker::{PickerStep, Selector};
use super::store::{self, Reason, SavedLock};

/// `XDG_STATE_HOME` is process-wide, so the tests that move it take turns.
static STATE: Mutex<()> = Mutex::new(());

/// A scratch state directory and a checkout holding a lock: nothing here touches the real home.
fn with_scratch<T>(test: impl FnOnce(&str, &str) -> T) -> T {
    let _turn = STATE.lock().unwrap_or_else(|error| error.into_inner());
    let root: PathBuf = std::env::temp_dir().join(format!(
        "nix-rebuild-locks-{}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let state = root.join("state");
    let checkout = root.join("checkout");
    fs::create_dir_all(&state).expect("a state directory");
    fs::create_dir_all(&checkout).expect("a checkout");
    fs::write(checkout.join("flake.lock"), "{\"nodes\": {}}\n").expect("a lock file");
    let state = state.to_str().expect("utf-8 state path").to_owned();
    let checkout = checkout.to_str().expect("utf-8 checkout path").to_owned();

    let result = temp_env::with_var("XDG_STATE_HOME", Some(state.as_str()), || {
        test(&checkout, &state)
    });
    let _ = fs::remove_dir_all(&root);
    result
}

fn saved(id: &str) -> SavedLock {
    SavedLock {
        id: id.into(),
        checkout: "/repo".into(),
        reason: "update".into(),
        saved_at: 1_774_000_000,
        digest: "0123456789abcdef".into(),
    }
}

#[test]
fn an_update_keeps_the_lock_it_is_about_to_replace() {
    with_scratch(|checkout, _state| {
        let kept = store::save(checkout, Reason::Update).expect("the lock was kept");
        let listed = store::list();

        assert_eq!(listed.len(), 1, "one update keeps one lock");
        assert_eq!(listed[0].id, kept.id);
        assert_eq!(listed[0].checkout, checkout);
        assert_eq!(
            listed[0].reason_label(),
            "update",
            "the row says why it was kept"
        );
        assert_eq!(
            fs::read_to_string(store::locks_dir().join(&kept.id).join("flake.lock")).unwrap(),
            fs::read_to_string(std::path::Path::new(checkout).join("flake.lock")).unwrap(),
            "the kept file is the lock the update replaced, byte for byte"
        );
        assert_eq!(
            listed[0].digest,
            crate::common::records::digest(b"{\"nodes\": {}}\n"),
            "and the row carries the digest of those bytes"
        );

        let listing = store::listing();
        assert!(
            listing.starts_with("WHEN"),
            "the table has a heading: {listing}"
        );
        assert!(
            listing.contains(&kept.id) && listing.contains("update"),
            "the kept lock is listed with its reason:\n{listing}"
        );
        assert!(
            listing.ends_with("checkout\n"),
            "the last column names the checkout the lock pins:\n{listing}"
        );
        let head: String = listed[0].digest.chars().take(8).collect();
        assert!(
            listing.contains(&head),
            "the table shows the digest's head, so a run can be matched to it:\n{listing}"
        );
    });
}

#[test]
fn a_restore_puts_the_old_lock_back_and_keeps_the_one_it_replaced() {
    with_scratch(|checkout, _state| {
        let target = store::save(checkout, Reason::Update).expect("the first lock was kept");
        // The update that follows moves the lock on, and a rollback has to undo that.
        fs::write(
            std::path::Path::new(checkout).join("flake.lock"),
            "{\"nodes\": {\"new\": {}}}\n",
        )
        .expect("a newer lock");

        let restored = store::restore(&target.id, checkout).expect("the kept lock goes back");
        assert_eq!(restored.lock.id, target.id);
        assert!(
            restored.kept.is_some(),
            "the lock it replaced is kept, so the rollback can be undone"
        );
        assert_eq!(
            fs::read_to_string(std::path::Path::new(checkout).join("flake.lock")).unwrap(),
            "{\"nodes\": {}}\n",
            "the checkout carries the kept lock again"
        );

        let listed = store::list();
        assert_eq!(
            listed.len(),
            2,
            "the lock the rollback replaced was kept too"
        );
        assert_eq!(
            listed[0].reason_label(),
            "rollback",
            "and it says which verb replaced it"
        );
        assert_eq!(
            fs::read_to_string(store::locks_dir().join(&listed[0].id).join("flake.lock")).unwrap(),
            "{\"nodes\": {\"new\": {}}}\n",
            "so the rollback can itself be undone"
        );

        // `last` names the newest lock, the rollback's own copy included.
        assert_eq!(store::meta("last").unwrap().id, listed[0].id);
        assert_eq!(
            store::restore("last", checkout).unwrap().lock.id,
            listed[0].id,
            "last is the newest kept lock"
        );
    });
}

#[test]
fn a_lock_from_another_checkout_is_refused() {
    with_scratch(|checkout, state| {
        let target = store::save(checkout, Reason::Update).expect("the lock was kept");
        let other = std::path::Path::new(state).join("../other");
        fs::create_dir_all(&other).expect("another checkout");
        fs::write(other.join("flake.lock"), "{\"nodes\": {}}\n").expect("its own lock");
        let other = other.to_str().unwrap();

        let refused = store::restore(&target.id, other);
        assert!(
            refused.is_err(),
            "a lock only means something for the flake it pins"
        );
        assert!(
            refused.unwrap_err().to_string().contains("--flake"),
            "the refusal names the way out"
        );
        assert_eq!(
            fs::read_to_string(std::path::Path::new(other).join("flake.lock")).unwrap(),
            "{\"nodes\": {}}\n",
            "and the other checkout is untouched"
        );

        assert!(
            store::restore("nothing-by-that-name", checkout).is_err(),
            "an unknown id is a refusal, not a panic"
        );
        assert!(
            store::restore("../escape", checkout).is_err(),
            "an id is one directory name"
        );
    });
}

#[test]
fn pruning_keeps_the_newest_and_never_empties_the_store() {
    with_scratch(|checkout, _state| {
        for _ in 0..3 {
            store::save(checkout, Reason::Update).expect("a lock was kept");
        }
        let newest = store::list()[0].id.clone();

        store::prune(2);
        let kept = store::list();
        assert_eq!(kept.len(), 2, "only the newest two survive");
        assert_eq!(kept[0].id, newest, "the newest is one of them");

        store::prune(0);
        assert_eq!(
            store::list().len(),
            1,
            "a count of zero still keeps one, so the store never becomes empty by accident"
        );
    });
}

#[test]
fn the_selector_opens_the_lock_under_the_mark_once_the_question_is_answered() {
    let mut selector = Selector::new(vec![saved("a"), saved("b"), saved("c")], None);
    let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

    // Enter asks; the lock goes back on `y`, and nothing is written before that.
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(
        selector.asking(),
        Some(0),
        "the mark is what Enter asks about"
    );
    assert_eq!(
        selector.press(press(KeyCode::Char('y'))),
        PickerStep::Open("a".into())
    );
    assert_eq!(
        selector.asking(),
        None,
        "the question is answered, not left open"
    );

    // Esc and `n` keep the current lock, and Enter confirms the way `y` does.
    assert_eq!(selector.press(press(KeyCode::Down)), PickerStep::Continue);
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(selector.press(press(KeyCode::Esc)), PickerStep::Continue);
    assert_eq!(
        selector.asking(),
        None,
        "Esc drops the question, not the selector"
    );
    assert_eq!(selector.press(press(KeyCode::Down)), PickerStep::Continue);
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(
        selector.press(press(KeyCode::Char('n'))),
        PickerStep::Continue
    );
    assert_eq!(selector.asking(), None);
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(
        selector.press(press(KeyCode::Enter)),
        PickerStep::Open("c".into())
    );

    assert_eq!(
        selector.press(press(KeyCode::Down)),
        PickerStep::Continue,
        "the mark stops at the last row"
    );
    assert_eq!(selector.press(press(KeyCode::Home)), PickerStep::Continue);
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(
        selector.press(press(KeyCode::Char('Y'))),
        PickerStep::Open("a".into())
    );

    // The arrows do not move the mark while the question is open.
    assert_eq!(selector.press(press(KeyCode::Enter)), PickerStep::Continue);
    assert_eq!(selector.press(press(KeyCode::Down)), PickerStep::Continue);
    assert_eq!(selector.selected(), 0, "the question freezes the list");
    assert_eq!(selector.press(press(KeyCode::Esc)), PickerStep::Continue);

    assert_eq!(selector.press(press(KeyCode::Esc)), PickerStep::Leave);
    assert_eq!(selector.press(press(KeyCode::Char('q'))), PickerStep::Leave);

    let mut empty = Selector::new(Vec::new(), None);
    assert_eq!(
        empty.press(press(KeyCode::Enter)),
        PickerStep::Leave,
        "Enter with nothing kept leaves instead of opening nothing"
    );
}

#[test]
fn the_selector_draws_one_row_per_lock_with_the_marked_one_first() {
    use super::picker::picker_to_string;

    let screen = picker_to_string(
        &[saved("20261029-134201-1a2b"), saved("20261028-101010-2b3c")],
        0,
        None,
        100,
        12,
    );
    let rows: Vec<&str> = screen.lines().collect();

    assert!(
        screen.contains("Saved locks"),
        "the frame names itself:\n{screen}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("WHEN") && row.contains("REASON")),
        "the heading names the columns:\n{screen}"
    );
    let marked = rows
        .iter()
        .find(|row| row.contains('❯'))
        .unwrap_or_else(|| panic!("no marked row:\n{screen}"));
    assert!(
        marked.contains("20261029-134201-1a2b"),
        "the first lock is the one under the mark:\n{screen}"
    );
    assert_eq!(
        rows.iter().filter(|row| row.contains("202610")).count(),
        2,
        "one row per kept lock:\n{screen}"
    );
    assert!(
        screen.lines().all(|line| line.chars().count() <= 100),
        "every drawn line fits the width it was given:\n{screen}"
    );
}

#[test]
fn the_settings_row_bounds_the_kept_locks() {
    use crate::common::config::Settings;

    let rows = super::settings_entries();
    assert_eq!(rows[0].section, "Locks");
    assert_eq!(rows[0].label, "Locks kept");

    let mut settings = Settings::default();
    assert_eq!(settings.keep_locks, 20, "twenty kept locks by default");
    rows[0].apply(&mut settings, -1);
    assert_eq!(settings.keep_locks, 19);

    settings.keep_locks = 1;
    rows[0].apply(&mut settings, -1);
    assert_eq!(settings.keep_locks, 1, "the row cannot go below one");
}

#[test]
fn a_click_opens_the_lock_it_lands_on_and_the_wheel_moves_the_mark() {
    use super::picker::{PickerGeometry, lock_mouse};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut selector = Selector::new(vec![saved("a"), saved("b"), saved("c")], None);
    let geometry = PickerGeometry {
        first_row: 4,
        rows: 3,
        offset: 0,
        total: 3,
        confirm: None,
    };

    assert_eq!(geometry.row_at(3), None, "the heading is not a row");
    assert_eq!(geometry.row_at(4), Some(0));
    assert_eq!(geometry.row_at(6), Some(2));
    assert_eq!(
        geometry.row_at(7),
        None,
        "the blank below the rows is not one"
    );

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        lock_mouse(&mut selector, &geometry, click),
        PickerStep::Continue,
        "a click asks, the same as Enter under the mark"
    );
    assert_eq!(
        selector.asking(),
        Some(1),
        "the clicked row is the one asked about"
    );
    // The pointer is cold while the question is open, so a stray click cannot answer it.
    assert_eq!(
        lock_mouse(&mut selector, &geometry, click),
        PickerStep::Continue
    );
    assert_eq!(
        selector.asking(),
        Some(1),
        "the question survives the click"
    );
    let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
    assert_eq!(
        selector.press(press(KeyCode::Char('y'))),
        PickerStep::Open("b".into()),
        "the click then `y` puts the lock under the pointer back"
    );

    selector.mark_row(0);
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        ..click
    };
    assert_eq!(
        lock_mouse(&mut selector, &geometry, wheel),
        PickerStep::Continue
    );
    assert_eq!(
        selector.selected(),
        2,
        "the wheel moves three rows and stops"
    );
}

#[test]
fn the_lock_the_checkout_holds_is_marked_and_named() {
    use super::picker::picker_to_string;

    let mut first = saved("a");
    first.digest = "9382a635aaaaaaaa".into();
    let mut second = saved("b");
    second.digest = "41f1002ebbbbbbbb".into();
    let locks = [first, second.clone()];

    // `b` is what the checkout holds: its row carries the bullet and the card above says which.
    let screen = picker_to_string(&locks, 0, Some(&second.digest), 100, 14);
    let rows: Vec<&str> = screen.lines().collect();
    assert!(
        screen.contains("41f1002e  ·  the lock kept by the update of"),
        "the state card names the digest and which kept lock it is\ngot\n{screen}"
    );
    assert!(
        screen.contains("\u{256d}\u{2500} Now "),
        "and it is a card of its own, titled like the rebuild's\ngot\n{screen}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains('\u{2022}') && row.contains("41f1002e")),
        "the row holding that digest is marked\ngot\n{screen}"
    );
    assert!(
        rows.iter()
            .filter(|row| row.contains('\u{2022}'))
            .all(|row| row.contains("41f1002e")),
        "and only that one\ngot\n{screen}"
    );

    // When nothing kept matches, the card says so instead of pointing at a row that is not there.
    let screen = picker_to_string(&locks, 0, Some("ffffffffffffffff"), 100, 14);
    assert!(
        screen.contains("ffffffff  ·  no saved lock holds it"),
        "got\n{screen}"
    );
    assert!(
        !screen.contains('\u{2022}'),
        "no row is marked\ngot\n{screen}"
    );

    // A checkout with no lock to read still says where it stands.
    let screen = picker_to_string(&locks, 0, None, 100, 14);
    assert!(screen.contains("no flake.lock to read"), "got\n{screen}");
}

#[test]
fn the_question_says_which_digest_leaves_and_which_arrives() {
    use super::picker::picker_to_question_string;

    let mut chosen = saved("a");
    chosen.digest = "9382a635aaaaaaaa".into();
    let mut held = saved("b");
    held.digest = "41f1002ebbbbbbbb".into();
    let locks = [chosen.clone(), held.clone()];

    let screen = picker_to_question_string(&locks, 0, Some(&held.digest), 100, 16);
    assert!(screen.contains("Revert the lock of"), "got\n{screen}");
    assert!(
        screen.contains("41f1002e → 9382a635"),
        "the question names both ends of the move\ngot\n{screen}"
    );
    assert!(
        screen.contains("[Y  Revert and rebuild]"),
        "the same answers `runs` gives for the same move: the two boxes are one\ngot\n{screen}"
    );
    assert!(
        screen.contains("[N  Keep the lock]"),
        "the answer that writes nothing is a chip, not a hint\ngot\n{screen}"
    );
    assert!(
        screen.contains("[Esc]"),
        "the corner is the way out, so the box needs no legend\ngot\n{screen}"
    );
    assert!(
        !screen.contains("rebuilt on those inputs"),
        "the button already says what happens\ngot\n{screen}"
    );
    assert!(
        !screen.contains("can be undone"),
        "the box says the question, not what the store keeps\ngot\n{screen}"
    );
    assert!(
        screen.contains("╭ Rollback "),
        "the same box the rollback in `runs` draws\ngot\n{screen}"
    );

    // Choosing the lock that is already there is a rebuild that changes nothing, and says so.
    let screen = picker_to_question_string(&locks, 0, Some(&chosen.digest), 100, 16);
    assert!(
        screen.contains("9382a635 is already what the checkout holds."),
        "got\n{screen}"
    );
}

#[test]
fn the_questions_answers_take_a_click_like_a_key() {
    use super::picker::{lock_mouse, picker_to_question_geometry};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let locks = [saved("a"), saved("b")];
    let geometry = picker_to_question_geometry(&locks, 1, None, 100, 20);
    let answers = geometry.confirm.expect("the question drew its answers");
    let click = |column: u16, row: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    // The second row is the mark, and Enter is what puts the question up.
    let ask = |selector: &mut Selector| {
        selector.press(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            selector.press(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PickerStep::Continue
        );
        assert_eq!(selector.asking(), Some(1));
    };

    // `[N]` keeps the lock the checkout holds, which is the answer Esc gives.
    let mut selector = Selector::new(locks.to_vec(), None);
    ask(&mut selector);
    assert_eq!(
        lock_mouse(&mut selector, &geometry, click(answers.no.0, answers.no.1)),
        PickerStep::Continue
    );
    assert!(
        selector.asking().is_none(),
        "the chip puts the question down"
    );

    // `[Y]` puts the marked lock back and hands its id to the run.
    ask(&mut selector);
    assert_eq!(
        lock_mouse(
            &mut selector,
            &geometry,
            click(answers.yes.0, answers.yes.1)
        ),
        PickerStep::Open("b".into())
    );

    // A click anywhere else, the list behind it included, answers nothing.
    ask(&mut selector);
    assert_eq!(
        lock_mouse(&mut selector, &geometry, click(2, 6)),
        PickerStep::Continue
    );
    assert!(
        selector.asking().is_some(),
        "a stray click leaves the question up"
    );
}
