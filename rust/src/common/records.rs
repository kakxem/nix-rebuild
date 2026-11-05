//! The recorded run's data model and the table it is listed as: what the runs hold,
//! independent of where it is stored.

use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::pairs::pairs;
use crate::common::scope::Scope;

/// Written as `key = value` lines: readable without a JSON parser.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub id: String,
    pub scope: String,
    pub flake: String,
    /// `switch`, `boot` or `update`, so a reopened frame names it the same way.
    pub action: String,
    pub cleanup: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub cleanup_ok: Option<bool>,
    pub status: String,
    /// The lock file this run kept, when it kept one: what `R` in the runs selector puts back.
    pub kept: Option<String>,
    /// The digest of the `flake.lock` this run built with, read when it finished.
    pub lock: String,
}

impl Meta {
    /// Values that can contain a `#` are quoted: the reader treats an unquoted `#` as a comment.
    pub fn text(&self) -> String {
        let mut text = format!(
            "id = {}\nscope = {}\nflake = \"{}\"\naction = {}\ncleanup = {}\nstarted_at = {}\n",
            self.id, self.scope, self.flake, self.action, self.cleanup, self.started_at
        );
        if let Some(kept) = &self.kept {
            text.push_str(&format!("kept = {kept}\n"));
        }
        if !self.lock.is_empty() {
            text.push_str(&format!("lock = {}\n", self.lock));
        }
        if let Some(finished) = self.finished_at {
            text.push_str(&format!("finished_at = {finished}\n"));
        }
        if let Some(code) = self.exit_code {
            text.push_str(&format!("exit_code = {code}\n"));
        }
        if let Some(ok) = self.cleanup_ok {
            text.push_str(&format!("cleanup_ok = {ok}\n"));
        }
        text.push_str(&format!("status = \"{}\"\n", self.status));
        text
    }

    pub fn parse(text: &str) -> Self {
        let values = pairs(text);
        let get = |key: &str| values.get(key).cloned().unwrap_or_default();
        Self {
            id: get("id"),
            scope: get("scope"),
            flake: get("flake"),
            action: get("action"),
            cleanup: get("cleanup"),
            started_at: get("started_at").parse().unwrap_or_default(),
            finished_at: get("finished_at").parse().ok(),
            exit_code: get("exit_code").parse().ok(),
            cleanup_ok: match get("cleanup_ok").as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            status: get("status"),
            kept: values.get("kept").filter(|kept| !kept.is_empty()).cloned(),
            lock: get("lock"),
        }
    }

    pub fn elapsed(&self) -> Option<std::time::Duration> {
        let finished = self.finished_at?;
        Some(std::time::Duration::from_secs(
            finished.saturating_sub(self.started_at),
        ))
    }

    pub fn outcome(&self) -> String {
        match self.exit_code {
            None => "running".into(),
            Some(0) => "ok".into(),
            Some(code) => format!("exit {code}"),
        }
    }

    /// The recorded length as `MM:SS`, or a dash for a run that never wrote an end.
    pub fn duration(&self) -> String {
        match self.elapsed() {
            Some(elapsed) => clock(elapsed),
            None => "—".into(),
        }
    }

    pub fn when(&self) -> String {
        crate::common::time::when(self.started_at)
    }

    /// Searches the list's columns plus the status, which the list leaves out.
    pub fn matches(&self, query: &str) -> bool {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        columns(self)
            .into_iter()
            .chain([self.status.clone()])
            .any(|cell| cell.to_lowercase().contains(&needle))
    }

    pub fn scope(&self) -> Scope {
        match self.scope.as_str() {
            "system" => Scope::System,
            // Only a run that stopped inside the step that builds both
            // configurations records this; a run that got further ends in system scope.
            "both" => Scope::Both,
            _ => Scope::Home,
        }
    }

    /// The line `runs <ID> --logs` starts with: action before scope, so an `update` is never
    /// mistaken for the scope it activated, and the whole digest where the table has room for
    /// its head only.
    pub fn heading(&self) -> String {
        let mut text = format!(
            "{} · {} · {} · {}\ncheckout: {}\n",
            self.id,
            self.action,
            self.scope,
            self.outcome(),
            self.flake
        );
        if !self.lock.is_empty() {
            text.push_str(&format!("lock: {}\n", self.lock));
        }
        text.push('\n');
        text
    }

    pub fn policy(&self) -> CleanupPolicy {
        CleanupPolicy::from_stored(&self.cleanup)
    }
}

/// One recorded line: kind `O` is the child's output, `J` a Nix event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub json: bool,
    pub line: String,
}

/// Shared by the table and the selector; each fits its heading plus the sort arrow.
pub const WHEN_COLUMN: usize = 12;
pub const ID_COLUMN: usize = 21;
pub const ACTION_COLUMN: usize = 8;
pub const SCOPE_COLUMN: usize = 7;
pub const RESULT_COLUMN: usize = 8;
pub const DURATION_COLUMN: usize = 8;
/// The first characters of the lock digest: enough to tell two locks apart at a glance.
pub const LOCK_COLUMN: usize = 8;

pub const COLUMN_GAP: &str = "  ";

/// `MM:SS`, one shape for a duration wherever it is shown.
pub fn clock(duration: std::time::Duration) -> String {
    crate::common::text::format_clock(duration)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    When,
    Id,
    Action,
    Scope,
    Lock,
    Result,
    Duration,
    Checkout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Ascending,
    Descending,
}

impl Column {
    /// Every column, in draw order.
    pub const ALL: [Column; 8] = [
        Column::When,
        Column::Id,
        Column::Action,
        Column::Scope,
        Column::Lock,
        Column::Result,
        Column::Duration,
        Column::Checkout,
    ];

    /// Checkout returns 0: it is last and takes the width that is left.
    pub fn width(self) -> usize {
        match self {
            Column::When => WHEN_COLUMN,
            Column::Id => ID_COLUMN,
            Column::Action => ACTION_COLUMN,
            Column::Scope => SCOPE_COLUMN,
            Column::Lock => LOCK_COLUMN,
            Column::Result => RESULT_COLUMN,
            Column::Duration => DURATION_COLUMN,
            Column::Checkout => 0,
        }
    }

    pub fn heading(self) -> &'static str {
        match self {
            Column::When => "WHEN",
            Column::Id => "ID",
            Column::Action => "ACTION",
            Column::Scope => "SCOPE",
            Column::Lock => "LOCK",
            Column::Result => "RESULT",
            Column::Duration => "DURATION",
            Column::Checkout => "CHECKOUT",
        }
    }

    pub fn cell(self, meta: &Meta) -> String {
        match self {
            Column::When => meta.when(),
            Column::Id => fit(&meta.id, ID_COLUMN),
            Column::Action => fit(&meta.action, ACTION_COLUMN),
            Column::Scope => fit(&meta.scope, SCOPE_COLUMN),
            Column::Lock => head(&meta.lock, LOCK_COLUMN),
            Column::Result => fit(&meta.outcome(), RESULT_COLUMN),
            Column::Duration => fit(&meta.duration(), DURATION_COLUMN),
            Column::Checkout => checkout(meta),
        }
    }

    /// `When` compares as a time: a spelled-out date does not sort across months.
    /// Everything else compares as the text the column shows.
    pub fn compare(self, left: &Meta, right: &Meta) -> std::cmp::Ordering {
        match self {
            Column::When => left.started_at.cmp(&right.started_at),
            _ => self
                .cell(left)
                .to_lowercase()
                .cmp(&self.cell(right).to_lowercase()),
        }
    }
}

/// One cell per column. The status is left out on purpose: a sentence belongs in the run's frame.
pub fn columns(meta: &Meta) -> [String; 8] {
    Column::ALL.map(|column| column.cell(meta))
}

fn checkout(meta: &Meta) -> String {
    checkout_cell(&meta.flake, &std::env::var("HOME").unwrap_or_default())
}

/// Shortens a checkout under `home` to `~`; `home` is passed in so tests can drive it.
pub fn checkout_cell(flake: &str, home: &str) -> String {
    if !home.is_empty()
        && let Some(rest) = flake.strip_prefix(home)
    {
        return format!("~{rest}");
    }
    flake.to_owned()
}

pub fn padded_columns(meta: &Meta) -> [String; 8] {
    Column::ALL.map(|column| {
        let cell = column.cell(meta);
        match column {
            Column::Checkout => cell,
            _ => format!("{cell:<w$}", w = column.width()),
        }
    })
}

pub fn row(meta: &Meta) -> String {
    padded_columns(meta).join(COLUMN_GAP)
}

pub fn headings() -> String {
    Column::ALL
        .map(|column| match column {
            Column::Checkout => column.heading().to_owned(),
            _ => format!("{:<w$}", column.heading(), w = column.width()),
        })
        .join(COLUMN_GAP)
}

/// Separate cells so the selector can mark the column the list is ordered by.
pub fn heading_cells() -> [String; 8] {
    Column::ALL.map(|column| match column {
        Column::Checkout => column.heading().to_owned(),
        _ => format!("{:<w$}", column.heading(), w = column.width()),
    })
}

/// The table `runs --list` prints, from the rows it is given.
pub fn listing(runs: &[Meta]) -> String {
    if runs.is_empty() {
        return "No runs recorded yet.\n".into();
    }
    let mut text = headings();
    text.push('\n');
    for meta in runs {
        text.push_str(&row(meta));
        text.push('\n');
    }
    text
}

/// The first `width` characters: a digest is read by its head, so the tail it loses says nothing.
pub fn head(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
}

/// The digest of the `flake.lock` a flake reference points at, with any `#attr` dropped. `None`
/// when there is no lock to read, which a remote flake is not a failure for.
pub fn lock_digest(flake: &str) -> Option<String> {
    let checkout = flake.split('#').next().unwrap_or_default();
    let bytes = std::fs::read(std::path::Path::new(checkout).join("flake.lock")).ok()?;
    Some(digest(&bytes))
}

/// A short identity for a lock file's contents: two runs that read the same file read the same
/// value, so the table says which rebuilds share their inputs. FNV-1a is enough for an identity;
/// it is not a checksum, and nothing should compare it with `sha256`.
pub fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Keeps the end and prefixes `…`: a checkout's tail says more than its head.
fn fit(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length <= width {
        return value.to_owned();
    }
    let tail: String = value.chars().skip(length - width + 1).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A recorded run, as it is read back.
    fn finished(id: &str) -> Meta {
        Meta {
            id: id.into(),
            scope: "home".into(),
            flake: "/srv/checkouts/nixos-dotfiles#sample".into(),
            action: "switch".into(),
            cleanup: "ask".into(),
            started_at: 1_791_918_874,
            finished_at: Some(1_791_918_934),
            exit_code: Some(0),
            cleanup_ok: None,
            status: "ok".into(),
            kept: None,
            lock: String::new(),
        }
    }

    #[test]
    fn a_recorded_run_reads_the_same_in_the_table_the_selector_and_a_heading() {
        use Meta;

        let meta = Meta {
            id: "20261013-214512-1a2b".into(),
            scope: "home".into(),
            flake: "/srv/checkouts/nixos-dotfiles#sample".into(),
            action: "switch".into(),
            cleanup: "ask".into(),
            started_at: 1_774_000_000,
            finished_at: Some(1_774_000_120),
            exit_code: Some(1),
            cleanup_ok: None,
            status: "home manager switch exited with 1".into(),
            kept: None,
            lock: "0123456789abcdef".into(),
        };

        // When, id, action, scope, result, duration and checkout are the columns; the status is
        // not one.
        let cells = columns(&meta);
        assert_eq!(cells[1], "20261013-214512-1a2b");
        assert_eq!(cells[2], "switch", "the verb the run was started by");
        assert_eq!(cells[3], "home");
        assert_eq!(cells[4], "01234567", "the inputs it built with");
        assert_eq!(cells[5].trim(), "exit 1");
        assert_eq!(cells[6].trim(), "02:00", "the recorded length, as a clock");
        assert_eq!(cells[7], meta.flake, "a checkout outside the home is whole");
        assert!(
            !row(&meta).contains("exited with 1"),
            "the status is not in the list"
        );

        // A checkout under home is read from `~`.
        assert_eq!(
            checkout_cell("/home/user/.config/nixos-dotfiles#sample", "/home/user"),
            "~/.config/nixos-dotfiles#sample"
        );
        assert_eq!(
            checkout_cell("/home/user/.config/nixos-dotfiles#sample", "/home/other"),
            "/home/user/.config/nixos-dotfiles#sample",
            "another home is not this one's shorthand",
        );
        assert_eq!(
            checkout_cell("/etc/nixos#host", ""),
            "/etc/nixos#host",
            "with no home to shorten against, the path stands",
        );

        // The offset math is what lines the row up under the headings.
        let offset = WHEN_COLUMN + 2 + ID_COLUMN + 2 + ACTION_COLUMN + 2;
        let row = row(&meta);
        let headings = headings();
        assert_eq!(&headings[offset..offset + 5], "SCOPE");
        assert_eq!(&row[offset..offset + 5], "home ");
        assert!(headings.starts_with("WHEN"), "got {headings}");
        assert!(headings.ends_with("CHECKOUT"), "got {headings}");

        // Unshortened: a heading has no columns to fit.
        let heading = meta.heading();
        assert!(
            heading.starts_with("20261013-214512-1a2b · switch · home · exit 1\n"),
            "got {heading}"
        );
        assert!(
            heading.contains(&format!("checkout: {}", meta.flake)),
            "the heading spells the whole path out:\n{heading}"
        );
    }

    #[test]
    fn a_run_spells_its_date_out_for_a_list_that_is_read_at_a_glance() {
        // Pinned to UTC: `when()` reads the local zone, so the day it prints moves
        // with the machine's offset while the id's stamp does not.
        temp_env::with_var("TZ", Some("UTC"), || {
            let when = finished("20261013-221434-258d1").when();
            let shape: Vec<char> = when.chars().collect();
            assert_eq!(shape.len(), 12, "`13 Oct 19:14`, got {when}");
            assert!(shape[0..2].iter().all(char::is_ascii_digit), "got {when}");
            assert_eq!(shape[2], ' ');
            assert!(shape[3].is_ascii_uppercase(), "a month name, got {when}");
            assert_eq!(shape[6], ' ');
            assert!(
                shape[7..9].iter().all(char::is_ascii_digit)
                    && shape[9] == ':'
                    && shape[10..12].iter().all(char::is_ascii_digit),
                "a clock, got {when}"
            );

            // The same day and month as the id's stamp.
            let id = finished("20261013-221434-258d1").id;
            assert_eq!(
                &id[6..8],
                &shape[0..2].iter().collect::<String>(),
                "the day"
            );
        });
    }

    #[test]
    fn the_record_round_trips() {
        use Meta;

        let meta = Meta {
            id: "20261008-214733-a3f0".into(),
            scope: "home".into(),
            flake: "/home/user/.config/nixos-dotfiles#sample".into(),
            action: "switch".into(),
            cleanup: "ask".into(),
            started_at: 1_000,
            finished_at: Some(1_342),
            exit_code: Some(0),
            cleanup_ok: Some(true),
            status: "rebuild and cleanup completed".into(),
            kept: Some("20261008-214700-cccc".into()),
            lock: "fedcba9876543210".into(),
        };

        assert_eq!(
            Meta::parse(&meta.text()).kept.as_deref(),
            Some("20261008-214700-cccc"),
            "the lock the run kept is recorded with it"
        );
        assert_eq!(
            Meta::parse(&meta.text()).lock,
            "fedcba9876543210",
            "and so are the inputs it built with"
        );
        assert_eq!(
            Meta::parse(&meta.text()),
            meta,
            "the hash in the flake survives"
        );
        assert_eq!(meta.elapsed(), Some(Duration::from_secs(342)));
        assert_eq!(meta.outcome(), "ok");
        assert_eq!(meta.scope(), Scope::Home);
        assert_eq!(meta.policy(), CleanupPolicy::Ask);

        let running = Meta {
            exit_code: None,
            finished_at: None,
            cleanup_ok: None,
            ..meta.clone()
        };
        assert_eq!(running.outcome(), "running");
        assert_eq!(running.elapsed(), None);
    }
}
