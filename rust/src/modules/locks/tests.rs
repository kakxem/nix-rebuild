//! The saved locks: what a save keeps, what a restore puts back, the selector's keys, and the
//! Settings row that bounds the store.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
