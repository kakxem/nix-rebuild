//! The lock files an update or a rollback replaced, under `$XDG_STATE_HOME/nix-rebuild/locks/<id>`:
//! the `flake.lock` itself and the `meta.conf` that says which checkout it pins and why it was kept.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::pairs::pairs;
use crate::common::records::{self, checkout_cell};
use crate::common::state::state_base;
use crate::common::time::{backup_stamp, when};

/// Why a lock was kept: the one an update replaced, or the one a rollback replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Update,
    Rollback,
}

impl Reason {
    pub fn stored(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Rollback => "rollback",
        }
    }

    fn from_stored(text: &str) -> Self {
        match text {
            "rollback" => Self::Rollback,
            _ => Self::Update,
        }
    }
}

/// One kept lock, as its `meta.conf` reads back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedLock {
    pub id: String,
    pub checkout: String,
    pub reason: String,
    pub saved_at: u64,
    /// The digest of the kept `flake.lock`, the same one the run that built with it records.
    pub digest: String,
}

impl SavedLock {
    /// The checkout is quoted: a path can carry a `#` the reader would read as a comment.
    pub fn text(&self) -> String {
        let mut text = format!(
            "id = {}\ncheckout = \"{}\"\nreason = {}\nsaved_at = {}\n",
            self.id, self.checkout, self.reason, self.saved_at
        );
        if !self.digest.is_empty() {
            text.push_str(&format!("digest = {}\n", self.digest));
        }
        text
    }

    pub fn parse(text: &str) -> Self {
        let values = pairs(text);
        let get = |key: &str| values.get(key).cloned().unwrap_or_default();
        Self {
            id: get("id"),
            checkout: get("checkout"),
            reason: get("reason"),
            saved_at: get("saved_at").parse().unwrap_or_default(),
            digest: get("digest"),
        }
    }

    pub fn when(&self) -> String {
        when(self.saved_at)
    }

    /// A record without a reason predates the field, and every one of those came from an update.
    pub fn reason_label(&self) -> &'static str {
        Reason::from_stored(&self.reason).stored()
    }
}

/// `$XDG_STATE_HOME/nix-rebuild/locks`.
pub fn locks_dir() -> PathBuf {
    state_base().join("nix-rebuild/locks")
}

/// The lock directories, oldest first: the ids are stamps, so the name sorts.
fn dirs() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(locks_dir()) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Every kept lock, newest first.
pub fn list() -> Vec<SavedLock> {
    dirs()
        .iter()
        .rev()
        .filter_map(|dir| {
            Some(SavedLock::parse(
                &fs::read_to_string(dir.join("meta.conf")).ok()?,
            ))
        })
        .collect()
}

/// Keep the checkout's `flake.lock` as it stands. `None` when there is no lock to keep or
/// the state directory cannot be written: a lost backup must never stop a rebuild.
pub fn save(checkout: &str, reason: Reason) -> Option<SavedLock> {
    let contents = fs::read(Path::new(checkout).join("flake.lock")).ok()?;
    let id = next_id();
    let dir = locks_dir().join(&id);
    fs::create_dir_all(&dir).ok()?;
    fs::write(dir.join("flake.lock"), &contents).ok()?;
    let saved = SavedLock {
        id,
        checkout: checkout.to_owned(),
        reason: reason.stored().into(),
        saved_at: epoch_seconds(),
        digest: records::digest(&contents),
    };
    File::create(dir.join("meta.conf"))
        .ok()?
        .write_all(saved.text().as_bytes())
        .ok()?;
    Some(saved)
}

/// A stamp, a pid, and a counter when that pair is already taken: two saves in one second
/// share a process, and the second one must not overwrite the first.
fn next_id() -> String {
    let base = format!("{}-{:x}", backup_stamp(), std::process::id());
    let mut id = base.clone();
    let mut taken = 1;
    while locks_dir().join(&id).is_dir() {
        id = format!("{base}-{taken}");
        taken += 1;
    }
    id
}

/// What a restore did: the lock it put back, and the id of the one it kept in its place.
#[derive(Debug)]
pub struct Restored {
    pub lock: SavedLock,
    /// `None` when the lock being replaced could not be kept; the restore still went ahead.
    pub kept: Option<String>,
}

/// Put the lock named by `id` (or `last`) back on its checkout. The lock being replaced is
/// kept first, so a rollback is itself reversible.
pub fn restore(id_or_last: &str, checkout: &str) -> anyhow::Result<Restored> {
    let dir = dir_of(id_or_last).ok_or_else(|| {
        anyhow::anyhow!("no saved lock named `{id_or_last}`; `nix-rebuild locks --list` lists them")
    })?;
    let saved = SavedLock::parse(&fs::read_to_string(dir.join("meta.conf")).unwrap_or_default());
    if saved.checkout != checkout {
        anyhow::bail!(
            "that lock was kept for {}, not {checkout}: a lock only means something for the \
             flake it pins, so pass --flake {}",
            saved.checkout,
            saved.checkout
        );
    }
    let contents = fs::read(dir.join("flake.lock"))
        .map_err(|error| anyhow::anyhow!("read {}: {error}", dir.join("flake.lock").display()))?;
    let kept = save(checkout, Reason::Rollback);
    // A temporary file and a rename, so an interrupted write never leaves half a lock.
    let temp = Path::new(checkout).join("flake.lock.tmp");
    fs::write(&temp, contents)
        .map_err(|error| anyhow::anyhow!("write {}: {error}", temp.display()))?;
    fs::rename(&temp, Path::new(checkout).join("flake.lock"))
        .map_err(|error| anyhow::anyhow!("replace flake.lock: {error}"))?;
    Ok(Restored {
        lock: saved,
        kept: kept.map(|kept| kept.id),
    })
}

/// The directory a name points at: `last` for the newest, otherwise an id that exists.
fn dir_of(id_or_last: &str) -> Option<PathBuf> {
    if id_or_last == "last" {
        return dirs().pop();
    }
    // An id is one directory name: a restore must never reach outside the saved locks.
    if id_or_last.is_empty() || id_or_last == "." || id_or_last == ".." || id_or_last.contains('/')
    {
        return None;
    }
    let dir = locks_dir().join(id_or_last);
    dir.is_dir().then_some(dir)
}

/// One kept lock by name, without touching the checkout.
pub fn meta(id_or_last: &str) -> Option<SavedLock> {
    let dir = dir_of(id_or_last)?;
    Some(SavedLock::parse(
        &fs::read_to_string(dir.join("meta.conf")).ok()?,
    ))
}

/// The digest of the `flake.lock` the checkout holds now, so the selector can say where the
/// reader is standing instead of leaving them to compare eight hex characters from memory.
pub fn current(checkout: &str) -> Option<String> {
    records::lock_digest(checkout)
}

/// Drop all but the newest `keep`, so the store cannot grow without a bound. A lock is a few
/// kilobytes, so a count is the only ceiling this needs.
pub fn prune(keep: usize) {
    let mut dirs = dirs();
    let excess = dirs.len().saturating_sub(keep.max(1));
    for dir in dirs.drain(..excess) {
        let _ = fs::remove_dir_all(dir);
    }
}

pub const WHEN_COLUMN: usize = 12;
pub const ID_COLUMN: usize = 23;
pub const REASON_COLUMN: usize = 9;
pub const LOCK_COLUMN: usize = records::LOCK_COLUMN;

pub const COLUMN_GAP: &str = "  ";

pub fn heading_cells() -> [String; 5] {
    [
        format!("{:<when$}", "WHEN", when = WHEN_COLUMN),
        format!("{:<id$}", "ID", id = ID_COLUMN),
        format!("{:<reason$}", "REASON", reason = REASON_COLUMN),
        format!("{:<lock$}", "LOCK", lock = LOCK_COLUMN),
        "CHECKOUT".to_owned(),
    ]
}

pub fn headings() -> String {
    heading_cells().join(COLUMN_GAP)
}

/// The cells of a row, each padded to its width; the checkout is last and takes what is left,
/// shortened from `HOME`.
pub fn cells(lock: &SavedLock) -> [String; 5] {
    let home = std::env::var("HOME").unwrap_or_default();
    let digest: String = lock.digest.chars().take(LOCK_COLUMN).collect();
    [
        format!("{:<when$}", lock.when(), when = WHEN_COLUMN),
        format!("{:<id$}", fit(&lock.id), id = ID_COLUMN),
        format!("{:<reason$}", lock.reason_label(), reason = REASON_COLUMN),
        format!("{digest:<lock$}", lock = LOCK_COLUMN),
        checkout_cell(&lock.checkout, &home),
    ]
}

/// One row of the table `locks --list` prints and the selector draws.
pub fn row(lock: &SavedLock) -> String {
    cells(lock).join(COLUMN_GAP)
}

/// The table `locks --list` prints, from the locks it is given, newest first.
pub fn listing_of(locks: &[SavedLock]) -> String {
    if locks.is_empty() {
        return "No lock saved yet: `nix-rebuild update` keeps the one it replaces.\n".into();
    }
    let mut text = headings();
    text.push('\n');
    for lock in locks {
        text.push_str(&row(lock));
        text.push('\n');
    }
    text
}

pub fn listing() -> String {
    listing_of(&list())
}

/// Keeps the head: the date says which lock this is, the pid suffix does not.
fn fit(id: &str) -> String {
    if id.chars().count() <= ID_COLUMN {
        return id.to_owned();
    }
    let head: String = id.chars().take(ID_COLUMN - 1).collect();
    format!("{head}…")
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}
