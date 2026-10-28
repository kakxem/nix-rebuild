//! The Logs panel's state: the records, where the window looks, the filter, and the copy confirmation.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::common::hit_targets::{ScrollbarGeometry, row_target_at};
use crate::common::nix_events::{LogKind, LogLevel, NixEvent};

/// How far back (entries) and how soon after (age) an exact repeat folds into an existing line.
/// Generous on purpose: one rebuild prints the same derivations and paths again and again.
const DUPLICATE_WINDOW: usize = 20;
const DUPLICATE_WINDOW_AGE: Duration = Duration::from_secs(2);

/// Where the log panel looks: `Follow` tracks the tail, `Anchored` pins the window
/// so arriving records never move the text being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogCursor {
    #[default]
    Follow,
    Anchored,
}

impl LogCursor {
    pub fn is_anchored(self) -> bool {
        matches!(self, Self::Anchored)
    }
}

// Filtering is a view concern: records stay in the model, so clearing the filter restores the whole history.

/// One bit per semantic log kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindSet {
    bits: u8,
}

pub const FILTERABLE_KINDS: [LogKind; 8] = [
    LogKind::Error,
    LogKind::Warning,
    LogKind::Evaluation,
    LogKind::Download,
    LogKind::Build,
    LogKind::Activation,
    LogKind::Cleanup,
    LogKind::Info,
];

/// The filter dialog's rows on screen: query, its count, a blank, the raw toggle, one row per
/// kind, a blank and the save button.
pub const FILTER_ROWS: u16 = SAVE_ROW + 1;
/// The save button's line inside the dialog body.
pub const SAVE_ROW: u16 = FILTERABLE_KINDS.len() as u16 + 5;
/// The same button in the selection, which counts only the rows a pointer can land on.
pub const SAVE_SELECTION: usize = FILTERABLE_KINDS.len() + 2;

/// The word a kind is called by in the dialog and in the settings file.
pub fn kind_name(kind: LogKind) -> &'static str {
    match kind {
        LogKind::Error => "errors",
        LogKind::Warning => "warnings",
        LogKind::Evaluation => "evaluation",
        LogKind::Download => "fetch",
        LogKind::Build => "build",
        LogKind::Activation => "activate",
        LogKind::Cleanup => "cleanup",
        LogKind::Info => "info",
    }
}

impl KindSet {
    pub fn all() -> Self {
        Self { bits: 0b1111_1111 }
    }

    pub fn none() -> Self {
        Self { bits: 0 }
    }

    pub fn contains(self, kind: LogKind) -> bool {
        self.bits & Self::bit(kind) != 0
    }

    pub fn toggle(&mut self, kind: LogKind) {
        self.bits ^= Self::bit(kind);
    }

    pub fn is_all(self) -> bool {
        self.bits == 0b1111_1111
    }

    fn bit(kind: LogKind) -> u8 {
        let index = FILTERABLE_KINDS
            .iter()
            .position(|candidate| *candidate == kind)
            .unwrap_or(7);
        1 << index
    }
}

impl Default for KindSet {
    fn default() -> Self {
        Self::all()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogFilter {
    pub query: String,
    pub kinds: KindSet,
}

impl LogFilter {
    /// True when nothing is filtered; the panel title uses this to decide whether to report a count.
    pub fn is_inactive(&self) -> bool {
        self.query.trim().is_empty() && self.kinds.is_all()
    }

    /// The filter as the file spells it: the query, and the kinds kept on.
    pub fn saved(&self) -> (String, String) {
        let kinds = FILTERABLE_KINDS
            .iter()
            .filter(|kind| self.kinds.contains(**kind))
            .map(|kind| kind_name(*kind))
            .collect::<Vec<_>>()
            .join(",");
        (self.query.clone(), kinds)
    }

    /// A filter read back from the file; an absent half keeps the default it does not speak for.
    pub fn from_saved(query: Option<&str>, kinds: Option<&str>) -> Self {
        let mut filter = Self::default();
        if let Some(query) = query {
            filter.query = query.to_owned();
        }
        if let Some(kinds) = kinds {
            let saved: Vec<&str> = kinds.split(',').map(str::trim).collect();
            filter.kinds = KindSet::none();
            for kind in FILTERABLE_KINDS {
                if saved.contains(&kind_name(kind)) {
                    filter.kinds.toggle(kind);
                }
            }
        }
        filter
    }

    pub fn matches(&self, kind: LogKind, text: &str) -> bool {
        if !self.kinds.contains(kind) {
            return false;
        }
        let needle = self.query.trim().to_lowercase();
        needle.is_empty() || text.to_lowercase().contains(&needle)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub event: NixEvent,
    pub at: Instant,
    /// Monotonic position in the run. The cursor is expressed against this, not a row index,
    /// so an anchored window survives incoming records.
    pub sequence: u64,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub entries: VecDeque<LogEntry>,
    pub scroll: usize,
    /// Follow the tail or hold a user-chosen position; recorded here so the renderer and
    /// the key handler share one truth instead of inferring it from `log_scroll`.
    pub cursor: LogCursor,
    /// The newest record's sequence when the view was anchored; later arrivals are what the pending counter reports.
    pub anchored_end_sequence: Option<u64>,
    pub next_sequence: u64,
    /// Rows the panel can show, refreshed by the renderer; scrolling clamps against it
    /// so the view can never run past the oldest line.
    pub viewport: usize,
    /// Exact bars painted in the latest frame, used for track clicks and drag.
    pub scrollbar: Option<ScrollbarGeometry>,
    /// The structured log filter, applied at draw time.
    pub filter: LogFilter,
    /// True while the filter dialog's query field has focus.
    pub filter_editing: bool,
    /// Which row of the filter dialog is selected.
    pub filter_row: usize,
    /// Mouse hit rectangles for the query, raw mode, and semantic rows.
    pub filter_rows: Vec<(u16, u16, u16, u16)>,
    /// True once the dialog has written this filter to the file, until it is changed again.
    pub saved: bool,
    /// Set by the copy control; the event loop owns the clipboard write.
    pub copy_logs: bool,
    /// When the copy confirmation was armed, so the control can revert.
    pub copied_at: Option<Instant>,
    /// How long the copy confirmation stays on screen.
    pub copied_confirm: Duration,
}

impl State {
    /// A repeat of a recent line folds into the record already there.
    pub fn push_entry(&mut self, event: NixEvent) {
        if self.repeats_a_recent_line(&event) {
            return;
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.entries.push_back(LogEntry {
            event,
            at: Instant::now(),
            sequence,
        });
    }

    pub fn set_log_viewport(&mut self, rows: usize) {
        self.viewport = rows;
        self.clamp_log_scroll();
    }

    /// Where the visible window ends: one past the anchor sequence, or the live tail when following.
    /// Everything drawn and clamped derives from this, so arrivals cannot shift an anchored view.
    fn anchor_end_index(&self) -> usize {
        let entries: Vec<&LogEntry> = self.visible_logs().collect();
        match self.anchored_end_sequence {
            Some(sequence) => entries
                .iter()
                .rposition(|entry| entry.sequence <= sequence)
                .map_or(0, |index| index + 1),
            None => entries.len(),
        }
    }

    /// How far back the view may travel: once the oldest record sits at the viewport top, there is nothing left to show.
    fn log_scroll_limit(&self) -> usize {
        self.anchor_end_index().saturating_sub(self.viewport)
    }

    /// Re-apply the clamp after a filter change so the view can never point past what is visible.
    pub fn clamp_log_scroll(&mut self) {
        let limit = self.log_scroll_limit();
        self.scroll = self.scroll.min(limit);
        if self.scroll == 0 {
            self.follow_latest_logs();
        }
    }

    /// Move the view up (positive) or down (negative). Reaching the newest record returns to
    /// follow mode; any other position anchors, so later arrivals cannot move the text.
    pub fn scroll_logs_by(&mut self, lines: i64) {
        // Anchor first: the move is relative to the record being read, not to a tail that may grow.
        if !self.cursor.is_anchored() {
            self.anchored_end_sequence = self.entries.back().map(|entry| entry.sequence);
        }
        self.cursor = LogCursor::Anchored;
        let limit = self.log_scroll_limit() as i64;
        self.scroll = (self.scroll as i64 + lines).clamp(0, limit) as usize;
        if self.scroll == 0 {
            self.follow_latest_logs();
        }
    }

    pub fn set_log_scroll(&mut self, offset: usize) {
        let target = offset.min(self.log_scroll_limit());
        self.scroll_logs_by(target as i64 - self.scroll as i64);
    }

    pub fn follow_latest_logs(&mut self) {
        self.cursor = LogCursor::Follow;
        self.anchored_end_sequence = None;
        self.scroll = 0;
    }

    /// Records that arrived since the view was anchored; zero while following.
    pub fn pending_log_count(&self) -> usize {
        match self.anchored_end_sequence {
            Some(sequence) => self
                .entries
                .iter()
                .filter(|entry| entry.sequence > sequence)
                .count(),
            None => 0,
        }
    }

    /// The visible sequences in display order. Test-facing, so a scroll assertion does not re-implement the window arithmetic.
    pub fn visible_log_sequence_window(&self) -> Vec<u64> {
        self.log_window_entries()
            .into_iter()
            .map(|entry| entry.sequence)
            .collect()
    }

    /// The records the panel draws, oldest first; the window ends `log_scroll` rows above the anchor, so arrivals never move it.
    pub fn log_window_entries(&self) -> Vec<&LogEntry> {
        let entries: Vec<&LogEntry> = self.visible_logs().collect();
        let end = self
            .anchor_end_index()
            .saturating_sub(self.scroll)
            .min(entries.len());
        let start = end.saturating_sub(self.viewport);
        entries[start..end].to_vec()
    }

    /// How many records the cursor can travel back through: the tail when following, the anchor when held.
    pub fn log_content_len(&self) -> usize {
        self.anchor_end_index()
    }

    /// The whole log history, oldest first, regardless of what is on screen.
    pub fn logs_for_clipboard(&self) -> String {
        self.entries
            .iter()
            .map(|entry| entry.event.raw().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn arm_copied_marker(&mut self, window: Duration) {
        self.copied_at = Some(Instant::now());
        self.copied_confirm = window;
    }

    pub fn clear_copied_marker(&mut self) {
        self.copied_at = None;
    }

    pub fn copied_confirmation_expired(&self) -> bool {
        self.copied_at
            .is_some_and(|at| at.elapsed() >= self.copied_confirm)
    }

    /// True while the copy control shows its confirmation instead of the clipboard glyph.
    pub fn showing_copied(&self) -> bool {
        self.copied_at.is_some() && !self.copied_confirmation_expired()
    }

    pub fn filter_row_at(&self, column: u16, row: u16) -> Option<usize> {
        row_target_at(&self.filter_rows, column, row)
    }

    /// Whether this exact line is a repeat to fold away. Non-errors fold within the duplicate window;
    /// errors match anywhere in the run, since Nix reports one failure twice (child output and a re-raised JSON event).
    pub(crate) fn repeats_a_recent_line(&self, event: &NixEvent) -> bool {
        if event.level == LogLevel::Error {
            return self.entries.iter().any(|entry| {
                entry.event.level == LogLevel::Error && entry.event.raw() == event.raw()
            });
        }
        self.entries
            .iter()
            .rev()
            .take(DUPLICATE_WINDOW)
            .any(|entry| {
                entry.at.elapsed() < DUPLICATE_WINDOW_AGE
                    && entry.event.raw() == event.raw()
                    && entry.event.level == event.level
            })
    }

    pub fn visible_logs(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries
            .iter()
            .filter(|entry| self.filter.matches(entry.event.kind(), entry.event.raw()))
    }
}
