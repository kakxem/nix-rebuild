//! Recorded runs under `$XDG_STATE_HOME/nix-rebuild/runs/<id>` (`meta.conf`, `run.log`).
//! The recorder writes what the frame hands it; reading one back is `App::reopen`'s job.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::records::{Meta, Record};
use crate::common::scope::Scope;
use crate::common::time::backup_stamp;

/// Ceiling for every recorded run: a few 150 MB updates would otherwise fill the home.
pub const MAX_RECORDED_BYTES: u64 = 256 * 1024 * 1024;

/// How a run ended, as the recorder writes it: the frame hands this over, the records own it.
pub struct Outcome {
    pub scope: Scope,
    pub action: String,
    pub exit_code: i32,
    pub status: String,
}

pub struct Recording {
    dir: PathBuf,
    meta: Meta,
    log: File,
}

impl Recording {
    /// Returns `None` when the home cannot be written: losing the record must never stop a rebuild.
    pub fn start(scope: Scope, flake: &str, action: &str, policy: CleanupPolicy) -> Option<Self> {
        let id = format!("{}-{:x}", backup_stamp(), std::process::id());
        let dir = runs_dir().join(&id);
        fs::create_dir_all(&dir).ok()?;
        let log = File::create(dir.join("run.log")).ok()?;
        let meta = Meta {
            id,
            scope: scope.to_string(),
            flake: flake.to_owned(),
            action: action.to_owned(),
            cleanup: policy.stored().into(),
            started_at: epoch_seconds(),
            status: "running".into(),
            ..Meta::default()
        };
        let recording = Self { dir, meta, log };
        recording.write_meta();
        Some(recording)
    }

    pub fn id(&self) -> &str {
        &self.meta.id
    }

    /// Unbuffered on purpose: a buffer would lose the tail of a run killed mid-run.
    pub fn record(&mut self, kind: char, line: &str) {
        let _ = writeln!(self.log, "{kind}\t{line}");
    }

    pub fn finish(&mut self, outcome: Outcome, cleanup_ok: Option<bool>) {
        self.meta.finished_at = Some(epoch_seconds());
        // A multi-step run is recorded under the step it ended on, not the one it started with.
        self.meta.scope = outcome.scope.to_string();
        self.meta.action = outcome.action;
        self.meta.exit_code = Some(outcome.exit_code);
        self.meta.cleanup_ok = cleanup_ok;
        self.meta.status = outcome.status;
        self.write_meta();
    }

    fn write_meta(&self) {
        let _ = fs::write(self.dir.join("meta.conf"), self.meta.text());
    }

    /// A failed pass leaves the plain log in place: losing the compression is cheaper than the record.
    pub fn seal(self) {
        let Self { dir, log, .. } = self;
        drop(log);
        let plain = dir.join("run.log");
        if !plain.is_file() {
            return;
        }
        let compressed = dir.join("run.log.gz");
        let temp = dir.join("run.log.gz.tmp");
        let _ = fs::remove_file(&temp);
        let compressed_ok = (|| -> std::io::Result<()> {
            let mut input = File::open(&plain)?;
            let mut encoder = GzEncoder::new(File::create(&temp)?, Compression::default());
            std::io::copy(&mut input, &mut encoder)?;
            encoder.finish()?;
            fs::rename(&temp, &compressed)?;
            Ok(())
        })();
        if compressed_ok.is_ok() {
            let _ = fs::remove_file(&plain);
        } else {
            let _ = fs::remove_file(&temp);
        }
    }
}

/// Where the state directory lives: `$XDG_STATE_HOME`, else `~/.local/state`.
fn state_base() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|_| PathBuf::from(".local/state"))
}

/// `$XDG_STATE_HOME/nix-rebuild/runs`, where a new recording is written.
pub fn runs_dir() -> PathBuf {
    state_base().join("nix-rebuild/runs")
}

/// Where recordings went before the rename: read and pruned, never written.
fn legacy_dir() -> PathBuf {
    state_base().join("nix-rebuild/sessions")
}

/// The run directories, oldest first, from both names: an id recorded under `sessions/` keeps
/// working, and two directories with the same id resolve to the current one.
fn dirs() -> Vec<PathBuf> {
    let mut found: std::collections::BTreeMap<std::ffi::OsString, PathBuf> = Default::default();
    for root in [legacy_dir(), runs_dir()] {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for path in entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
        {
            if let Some(name) = path.file_name() {
                found.insert(name.to_owned(), path);
            }
        }
    }
    found.into_values().collect()
}

/// Every recorded run, newest first.
pub fn list() -> Vec<Meta> {
    dirs()
        .iter()
        .rev()
        .filter_map(|dir| {
            Some(Meta::parse(
                &fs::read_to_string(dir.join("meta.conf")).ok()?,
            ))
        })
        .collect()
}

/// How many runs are recorded, counted without reading their headers.
pub fn count() -> usize {
    dirs().len()
}

/// What the records hold, for the Settings dialog: how many runs and how many bytes.
pub fn measured() -> (usize, u64) {
    let dirs = dirs();
    let bytes = dirs.iter().map(|dir| dir_size(dir)).sum();
    (dirs.len(), bytes)
}

/// The runs' rows in the Settings dialog: what is recorded, how it prunes, and how it is cut.
pub fn settings_entries() -> Vec<crate::common::config::SettingEntry> {
    use crate::common::config::{Reading, SettingEntry};

    vec![
        SettingEntry::figure("Runs", "Recorded runs", Reading::RecordedRuns),
        SettingEntry::figure("Runs", "Size on disk", Reading::RecordedBytes),
        SettingEntry::toggle(
            "Runs",
            "Drop old logs",
            |settings| settings.prune_logs,
            |settings| settings.prune_logs = !settings.prune_logs,
        ),
        SettingEntry::number(
            "Runs",
            "Logs kept",
            |settings| settings.keep_logs,
            |settings, delta| {
                let step = if delta < 0 { -1 } else { 1 };
                settings.keep_logs = (settings.keep_logs as i64 + step).clamp(1, 500) as usize;
            },
        ),
        // The button asks before cutting: the question names how many runs would go.
        SettingEntry::action(
            "Runs",
            "Remove all previous runs",
            Command::RemovePrevious.into(),
        ),
    ]
}

/// The recorded runs, as the Settings dialog and its question read them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    /// How many runs the question would delete, frozen when it was opened.
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// Ask before the runs are cut.
    RemovePrevious,
    /// The answer to that question: delete them, keeping the one this frame is writing.
    ConfirmRemove,
}

/// What the slice asks of the runtime; the reducer never touches the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Open the question; the runtime counts the runs before it is asked.
    AskRemove,
    Remove,
}

pub fn reduce(command: Command) -> Vec<Effect> {
    match command {
        Command::RemovePrevious => vec![Effect::AskRemove],
        Command::ConfirmRemove => vec![Effect::Remove],
    }
}

/// The question answers y/n and nothing else; Enter and Esc decline, as the cleanup's does.
pub fn confirm_key(key: crossterm::event::KeyEvent) -> Option<crate::common::actions::Command> {
    use crate::common::actions::Command as Envelope;
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Char('y' | 'Y') => Some(Command::ConfirmRemove.into()),
        KeyCode::Char('n' | 'N') | KeyCode::Esc | KeyCode::Enter => Some(Envelope::CloseOverlay),
        _ => None,
    }
}

/// The exit-screen row: this slice owns what the terminal is left with.
pub fn exit_settings_entry() -> crate::common::config::SettingEntry {
    use crate::common::config::{LeaveScreen, SettingEntry};

    SettingEntry::choice(
        "Terminal",
        "Terminal on exit",
        |settings| match settings.leave {
            LeaveScreen::Clean => "clean".into(),
            LeaveScreen::Frame => "keep the frame".into(),
        },
        |settings| {
            settings.leave = match settings.leave {
                LeaveScreen::Clean => LeaveScreen::Frame,
                LeaveScreen::Frame => LeaveScreen::Clean,
            }
        },
    )
}

pub fn prune(keep: usize) {
    prune_to(keep, MAX_RECORDED_BYTES);
}

/// Drop all but the newest `keep`, then the oldest runs until the records fit `max_bytes`.
fn prune_to(keep: usize, max_bytes: u64) {
    let mut dirs = dirs();
    let excess = dirs.len().saturating_sub(keep);
    for dir in dirs.drain(..excess) {
        let _ = fs::remove_dir_all(dir);
    }
    let mut total: u64 = dirs.iter().map(|dir| dir_size(dir)).sum();
    while total > max_bytes && dirs.len() > 1 {
        let oldest = dirs.remove(0);
        total = total.saturating_sub(dir_size(&oldest));
        let _ = fs::remove_dir_all(&oldest);
    }
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.metadata().ok())
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or(0)
}

/// The recording, plain or compressed: a run killed before it could seal leaves it plain.
fn read_log(dir: &Path) -> Option<String> {
    if let Ok(text) = fs::read_to_string(dir.join("run.log")) {
        return Some(text);
    }
    let mut text = String::new();
    let file = File::open(dir.join("run.log.gz")).ok()?;
    GzDecoder::new(file).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The directory a name points at: `last` for the newest, otherwise an id that exists.
fn dir_of(id_or_last: &str) -> Option<PathBuf> {
    if id_or_last == "last" {
        return dirs().pop();
    }
    // An id is one directory name: `--delete` must never reach outside the records.
    if id_or_last.is_empty() || id_or_last == "." || id_or_last == ".." || id_or_last.contains('/')
    {
        return None;
    }
    let current = runs_dir().join(id_or_last);
    if current.is_dir() {
        return Some(current);
    }
    let legacy = legacy_dir().join(id_or_last);
    legacy.is_dir().then_some(legacy)
}

/// The recorded header alone, without reading the log.
pub fn meta(id_or_last: &str) -> Option<Meta> {
    let dir = dir_of(id_or_last)?;
    Some(Meta::parse(
        &fs::read_to_string(dir.join("meta.conf")).ok()?,
    ))
}

/// Delete one recorded run. `false` means there was nothing by that name.
pub fn remove(id_or_last: &str) -> bool {
    dir_of(id_or_last).is_some_and(|dir| fs::remove_dir_all(dir).is_ok())
}

/// Delete every recorded run, returning how many went.
pub fn remove_all() -> usize {
    remove_all_except(None)
}

/// Delete every recorded run but `keep`, so the frame's own recording survives its button.
pub fn remove_all_except(keep: Option<&str>) -> usize {
    dirs()
        .into_iter()
        .filter(|dir| dir.file_name().and_then(|name| name.to_str()) != keep)
        .filter(|dir| fs::remove_dir_all(dir).is_ok())
        .count()
}

/// Read a run back: `last` for the newest, otherwise an id from the list.
pub fn load(id_or_last: &str) -> Option<(Meta, Vec<Record>)> {
    let dir = dir_of(id_or_last)?;
    let meta = Meta::parse(&fs::read_to_string(dir.join("meta.conf")).ok()?);
    let records = read_log(&dir)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (kind, rest) = line.split_once('\t')?;
            Some(Record {
                json: kind == "J",
                line: rest.to_owned(),
            })
        })
        .collect();
    Some((meta, records))
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// The table `runs --list` prints, read from the records.
pub fn listing() -> String {
    crate::common::records::listing(&list())
}

mod closing;
mod dialog;

pub use dialog::render_remove_dialog;

pub use closing::{
    Leave, closing_line, closing_text, leave_plan, print_closing_line, render_closing,
};

#[cfg(test)]
mod tests;
