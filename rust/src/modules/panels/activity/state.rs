//! The Activity panel's state: the rows, the dedup that folds Nix's repeats, and the scroll.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::hit_targets::{ScrollbarGeometry, row_target_at};
use crate::common::nix_events::store_name;

const MAX_FINISHED: usize = 400;

/// The `actFileTransfer` activity: a URL being read, with no store path to name.
const FILE_TRANSFER: i64 = 101;

/// The `actBuilds` activity: the whole set of derivations, the only place Nix counts them.
const BUILD_SET: i64 = 104;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Build,
    /// An aggregate row counting the paths a source contributes, not one path.
    Source,
    /// One row per cache, counting the paths it was asked about.
    Probe,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Queued,
    Running,
    Done,
    Failed,
    /// The run ended before reaching this row; the row itself is not at fault.
    NotBuilt,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub id: u64,
    pub name: String,
    pub kind: ActivityKind,
    pub state: ActivityState,
    pub phase: String,
    pub units: u64,
    pub total: u64,
    pub started: Instant,
    pub active_at: Option<Instant>,
    pub ended: Option<Instant>,
}

impl Activity {
    pub fn elapsed_at(&self, now: Instant) -> Duration {
        let start = self.active_at.unwrap_or(self.started);
        self.ended.unwrap_or(now).saturating_duration_since(start)
    }

    pub fn fraction(&self) -> Option<f64> {
        if self.state == ActivityState::Done {
            return Some(1.0);
        }
        if self.total > 0 {
            return Some((self.units as f64 / self.total as f64).clamp(0.0, 0.99));
        }
        None
    }
}

/// The Activity filter dialog's rows: three toggles, a blank line, then the save button.
pub const ACTIVITY_FILTER_ROWS: u16 = 5;
/// The save button's line inside the dialog body.
pub const SAVE_ROW: u16 = 4;
/// The same button in the selection, which counts only the rows a pointer can land on.
pub const SAVE_SELECTION: usize = 3;

/// The Activity panel's rows in the Settings dialog: what its list starts with.
pub fn settings_entries() -> Vec<crate::common::config::SettingEntry> {
    use crate::common::config::SettingEntry;

    vec![
        SettingEntry::toggle(
            "Activity",
            "Downloads",
            |settings| settings.show_downloads,
            |settings| settings.show_downloads = !settings.show_downloads,
        ),
        SettingEntry::toggle(
            "Activity",
            "Builds",
            |settings| settings.show_builds,
            |settings| settings.show_builds = !settings.show_builds,
        ),
        SettingEntry::toggle(
            "Activity",
            "Finished work",
            |settings| settings.show_activity_history,
            |settings| settings.show_activity_history = !settings.show_activity_history,
        ),
        SettingEntry::toggle(
            "Activity",
            "Cleanup steps",
            |settings| settings.show_cleanup_progress,
            |settings| settings.show_cleanup_progress = !settings.show_cleanup_progress,
        ),
    ]
}

/// What starting a run reports for the run's own line: phase badge and status.
#[derive(Debug, Clone)]
pub struct Started {
    /// Whether the phase badge moves to building.
    pub building: bool,
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub active: BTreeMap<u64, Activity>,
    pub finished: VecDeque<Activity>,
    pub scroll: usize,
    pub viewport: usize,
    /// Bars painted last frame, for track clicks and drag.
    pub scrollbar: Option<ScrollbarGeometry>,
    /// Which row of the Activity filter dialog is selected.
    pub filter_row: usize,
    /// True once the dialog has written its choices to the file, until one of them changes.
    pub saved: bool,
    /// Mouse hit rectangles for the rows in the Activity filter.
    pub filter_rows: Vec<(u16, u16, u16, u16)>,
    pub acts: HashMap<u64, Value>,
    pub aliases: HashMap<u64, u64>,
    /// Row owning each subject (path, URL, source or cache); repeated announcements fold into it.
    pub owners: HashMap<String, u64>,
    /// Distinct paths counted per subject, so a repeat is not a second path.
    pub announced: HashMap<String, HashSet<String>>,
    /// Ids of the `actBuilds` activities, whose progress belongs to no row.
    pub sets: HashSet<u64>,
    /// Derivations done of how many the run asked for; a build row carries no numbers of its own.
    pub build_progress: Option<(u64, u64)>,
    /// Visible rows as of the last layout, so a new one can be told from a resize.
    last_visible: usize,
}

impl State {
    pub fn start_activity(&mut self, id: u64, name: &str, kind: &str) -> Started {
        let kind = match kind {
            "download" => ActivityKind::Download,
            "source" => ActivityKind::Source,
            "probe" => ActivityKind::Probe,
            _ => ActivityKind::Build,
        };
        // Nix names fetched trees `source`; relabel so it is not read as a package.
        let name = if kind == ActivityKind::Download && name == "source" {
            "source tree"
        } else {
            name
        };
        self.active.insert(
            id,
            Activity {
                id,
                name: name.into(),
                kind,
                // Source and probe rows are aggregates with no bytes to wait for.
                state: if kind == ActivityKind::Download {
                    ActivityState::Queued
                } else {
                    ActivityState::Running
                },
                phase: String::new(),
                units: 0,
                total: 0,
                started: Instant::now(),
                active_at: None,
                ended: None,
            },
        );
        // Source and probe work is not building; the phase badge stays put.
        let building = matches!(kind, ActivityKind::Build | ActivityKind::Download);
        let status = match kind {
            ActivityKind::Build => format!("building {name}"),
            ActivityKind::Download => format!("queued {name}"),
            ActivityKind::Source => format!("copying {name}"),
            ActivityKind::Probe => format!("querying {name}"),
        };
        Started { building, status }
    }

    /// The row an announcement belongs to: one its subject already owns, or a new one.
    pub fn claim_row(
        &mut self,
        id: u64,
        subject: &str,
        name: &str,
        kind: &str,
    ) -> (u64, Option<Started>) {
        if let Some(owner) = self.owners.get(subject).copied()
            && self.active.contains_key(&owner)
        {
            self.aliases.insert(id, owner);
            return (owner, None);
        }
        let started = self.start_activity(id, name, kind);
        self.aliases.insert(id, id);
        self.owners.insert(subject.to_owned(), id);
        (id, Some(started))
    }

    /// Keep the id of the whole set: top level, no parent, the only one counting all builds.
    pub fn note_set_start(&mut self, id: u64, kind: i64, value: &Value) {
        let parent = value.get("parent").and_then(Value::as_u64).unwrap_or(0);
        if kind == BUILD_SET && parent == 0 {
            self.sets.insert(id);
        }
    }

    /// Fold the set's `done of expected` into one pair; zero expected says nothing yet.
    pub fn note_set_progress(&mut self, id: u64, fields: &[Value]) {
        if !self.sets.contains(&id) {
            return;
        }
        let done = fields.first().and_then(Value::as_u64).unwrap_or(0);
        let expected = fields.get(1).and_then(Value::as_u64).unwrap_or(0);
        if expected > 0 {
            self.build_progress = Some((done, expected));
        }
    }

    /// Count one distinct path onto the row its subject owns; repeats do not add.
    pub fn count_announcement(&mut self, owner: u64, subject: &str, item: &str) {
        let announced = self.announced.entry(subject.to_owned()).or_default();
        announced.insert(item.to_owned());
        let count = announced.len() as u64;
        if let Some(activity) = self.active.get_mut(&owner) {
            activity.units = count;
        }
    }

    /// Close the aggregate rows at the first build: they have no `stop` of their own.
    /// A source announced again later starts a fresh row with a fresh count.
    pub fn finish_fetching(&mut self) {
        let ids: Vec<u64> = self
            .active
            .values()
            .filter(|activity| matches!(activity.kind, ActivityKind::Source | ActivityKind::Probe))
            .map(|activity| activity.id)
            .collect();
        for id in ids {
            self.close_activity(id, ActivityState::Done);
        }
    }

    pub fn close_activity(&mut self, id: u64, state: ActivityState) {
        if let Some(mut activity) = self.active.remove(&id) {
            // The subject is free for a later announcement; drop its stale owner.
            self.owners.retain(|_, owner| *owner != id);
            self.announced
                .retain(|subject, _| self.owners.contains_key(subject));
            activity.ended = Some(Instant::now());
            activity.state = state;
            if activity.kind == ActivityKind::Download && activity.total == 0 {
                activity.total = activity.units;
            }
            self.finished.push_back(activity);
            while self.finished.len() > MAX_FINISHED {
                self.finished.pop_front();
            }
        }
    }

    pub fn set_activity_viewport(&mut self, rows: usize, show_builds: bool, show_downloads: bool) {
        self.viewport = rows;
        let visible = self.visible_activities(show_builds, show_downloads).count();
        // A view parked off the newest end holds its rows: rows arriving would otherwise
        // push the offset's window along and the reader would lose what he was looking at.
        if self.scroll > 0 && visible > self.last_visible {
            self.scroll += visible - self.last_visible;
        }
        self.last_visible = visible;
        self.clamp_activity_scroll(show_builds, show_downloads);
    }

    fn activity_scroll_limit(&self, show_builds: bool, show_downloads: bool) -> usize {
        self.visible_activities(show_builds, show_downloads)
            .count()
            .saturating_sub(self.viewport)
    }

    fn clamp_activity_scroll(&mut self, show_builds: bool, show_downloads: bool) {
        let limit = self.activity_scroll_limit(show_builds, show_downloads);
        self.scroll = self.scroll.min(limit);
    }

    pub fn set_activity_scroll(&mut self, offset: usize, show_builds: bool, show_downloads: bool) {
        self.scroll = offset.min(self.activity_scroll_limit(show_builds, show_downloads));
    }

    /// Move the activity view up (positive) or down (negative). Zero is follow mode.
    pub fn scroll_activities_by(&mut self, rows: i64, show_builds: bool, show_downloads: bool) {
        let limit = self.activity_scroll_limit(show_builds, show_downloads) as i64;
        self.scroll = (self.scroll as i64 + rows).clamp(0, limit) as usize;
    }

    pub fn activity_filter_row_at(&self, column: u16, row: u16) -> Option<usize> {
        row_target_at(&self.filter_rows, column, row)
    }

    pub fn visible_activities(
        &self,
        show_builds: bool,
        show_downloads: bool,
    ) -> impl Iterator<Item = &Activity> {
        let visible = |activity: &&Activity| match activity.kind {
            ActivityKind::Build => show_builds,
            // Fetching covers sources and probes too, not just downloads.
            ActivityKind::Download | ActivityKind::Source | ActivityKind::Probe => show_downloads,
        };
        let mut ordered: Vec<&Activity> = self.finished.iter().filter(visible).collect();
        ordered.extend(chronological(
            self.active
                .values()
                .filter(|activity| activity.state == ActivityState::Running)
                .filter(visible),
        ));
        ordered.extend(chronological(
            self.active
                .values()
                .filter(|activity| activity.state == ActivityState::Queued)
                .filter(visible),
        ));
        ordered.into_iter()
    }

    /// Mark the named build row as failed, open or already in history.
    /// Nix names the derivation after the row closed, so history is searched too.
    pub fn mark_failure(&mut self, name: &str) {
        if let Some(id) = self
            .active
            .values()
            .find(|activity| activity.kind == ActivityKind::Build && activity.name == name)
            .map(|activity| activity.id)
        {
            self.close_activity(id, ActivityState::Failed);
            return;
        }
        if let Some(activity) = self
            .finished
            .iter_mut()
            .rev()
            .find(|activity| activity.kind == ActivityKind::Build && activity.name == name)
        {
            activity.state = ActivityState::Failed;
        }
    }

    /// Fold `copying '<source>/<path>' to the store` into the row its source owns.
    /// It is the only record of fetching before the first build.
    pub fn note_copy(&mut self, id: u64, text: &str) -> Option<Started> {
        let (source, path) = copy_message(text)?;
        let subject = format!("source:{source}");
        let (owner, started) = self.claim_row(id, &subject, &source, "source");
        self.count_announcement(owner, &subject, &path);
        started
    }
}

/// One group's rows in start order; the map they come from has no order of its own.
fn chronological<'a>(rows: impl Iterator<Item = &'a Activity>) -> Vec<&'a Activity> {
    let mut rows: Vec<&Activity> = rows.collect();
    rows.sort_by_key(|activity| (activity.started, activity.id));
    rows
}

/// What a transfer brings in and the name its row carries.
/// Substitutions carry a store path; a flake input fetch carries only a URL (a narinfo is `probe_host`).
pub(crate) fn transfer_subject(kind: i64, fields: &[Value]) -> Option<(String, String)> {
    let strings = || fields.iter().filter_map(Value::as_str);
    if let Some(path) = strings().find(|field| field.starts_with("/nix/store/")) {
        return Some((path.to_owned(), store_name(path)));
    }
    if kind != FILE_TRANSFER {
        return None;
    }
    strings()
        .find(|field| field.starts_with("http://") || field.starts_with("https://"))
        .map(|url| (url.to_owned(), url_name(url)))
}

/// The cache a `.narinfo` URL asks, when the URL is one: a probe, not a transfer.
/// The row belongs to the cache and counts the paths asked; the `.nar` beside it is a download.
pub(crate) fn probe_host(url: &str) -> Option<String> {
    let url = url.strip_suffix(".narinfo")?;
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty())
        .map(str::to_owned)
}

/// Source and path from `copying '<source>/<path>' to the store`, where a flake input is `«ref»/<path>`.
/// `None` for any other message.
pub(crate) fn copy_message(text: &str) -> Option<(String, String)> {
    let quoted = text
        .strip_prefix("copying '")?
        .strip_suffix("' to the store")?;
    match quoted
        .strip_prefix('«')
        .and_then(|rest| rest.split_once('»'))
    {
        Some((reference, path)) => Some((
            source_name(reference),
            path.trim_start_matches('/').to_owned(),
        )),
        // A local tree is announced by its own paths, so the files of a dirty
        // checkout make one row instead of a row per file.
        None => Some(("local files".to_owned(), quoted.to_owned())),
    }
}

/// A row's name for a flake input: `owner/repo shortrev`, the host, or the reference.
fn source_name(reference: &str) -> String {
    // A narHash is a checksum of the revision, not part of its identity.
    let reference = reference.split('?').next().unwrap_or(reference);
    if let Some(rest) = reference.split("://").nth(1) {
        return rest.split('/').next().unwrap_or(reference).to_owned();
    }
    let mut parts = reference.split('/');
    let head = parts.next().unwrap_or(reference);
    // `github:owner/repo/rev`: the owner rides on the scheme.
    let (owner, Some(repo)) = (head.rsplit(':').next().unwrap_or(head), parts.next()) else {
        return reference.to_owned();
    };
    if owner.is_empty() {
        return reference.to_owned();
    }
    match parts.next() {
        Some(rev) => format!("{owner}/{repo} {}", short_rev(rev)),
        None => format!("{owner}/{repo}"),
    }
}

/// A full hash shortens to eight characters; a branch or tag is kept as is.
fn short_rev(rev: &str) -> String {
    if rev.len() > 8 && rev.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        rev[..8].to_owned()
    } else {
        rev.to_owned()
    }
}

/// Readable name for a URL-addressed transfer: the host plus the tail of the path,
/// in the same `…/a/b` shape a store path is shown.
pub fn url_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    // A reference can carry credentials, which say nothing about the fetch.
    let host = host.rsplit('@').next().unwrap_or(host);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return host.to_owned();
    }
    if parts.len() > 2 {
        return format!(
            "{host}/…/{}/{}",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        );
    }
    format!("{host}/{}", parts.join("/"))
}
