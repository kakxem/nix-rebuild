//! The remembered settings, `$XDG_CONFIG_HOME/nix-rebuild/config.toml`: TOML, parsed by hand.
//! A missing, partial or unwritable file falls back to the defaults and never blocks a rebuild.

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::pairs::pairs;

pub mod entry;

pub use entry::{Reading, SettingEntry, SettingKind};

pub const DEFAULT_KEEP_GENERATIONS: usize = 5;
/// Runs the journal keeps before the oldest is dropped, unless the setting says otherwise.
pub const DEFAULT_KEEP_LOGS: usize = 20;

/// What the shell's own screen is left with when a frame closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveScreen {
    /// The frame is drawn again on the shell's screen, closing line included.
    Frame,
    /// Nothing of the frame is left. A run that just happened leaves its closing line
    /// under the prompt; a reopened run leaves nothing.
    Clean,
}

impl LeaveScreen {
    fn label(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Clean => "clean",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub cleanup: CleanupPolicy,
    pub keep_generations: usize,
    /// Drop the oldest recorded runs when a run finishes.
    pub prune_logs: bool,
    /// How many recorded runs stay when pruning.
    pub keep_logs: usize,
    /// The checkout rebuilt when no `--flake` is given.
    pub checkout: Option<String>,
    pub leave: LeaveScreen,
    pub show_status: bool,
    pub show_activity: bool,
    pub show_logs: bool,
    pub show_downloads: bool,
    pub show_builds: bool,
    pub show_activity_history: bool,
    pub raw_logs: bool,
    /// The Logs filter a start begins with, written by the filter dialog's save button.
    pub log_filter_query: Option<String>,
    pub log_filter_kinds: Option<String>,
    /// Whether the Activity card shows the cleanup steps instead of the history.
    pub show_cleanup_progress: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cleanup: CleanupPolicy::Ask,
            keep_generations: DEFAULT_KEEP_GENERATIONS,
            prune_logs: true,
            keep_logs: DEFAULT_KEEP_LOGS,
            checkout: None,
            leave: LeaveScreen::Clean,
            show_status: true,
            show_activity: true,
            show_logs: true,
            show_downloads: true,
            show_builds: true,
            show_activity_history: false,
            raw_logs: false,
            log_filter_query: None,
            log_filter_kinds: None,
            show_cleanup_progress: true,
        }
    }
}

impl Settings {
    /// Recognised `key = value` pairs; missing or unreadable ones keep their default.
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for (key, value) in pairs(text) {
            match key.as_str() {
                "cleanup" => {
                    settings.cleanup = match value.as_str() {
                        "always" | "yes" => CleanupPolicy::Always,
                        "never" | "off" => CleanupPolicy::Never,
                        _ => CleanupPolicy::Ask,
                    }
                }
                "keep_generations" => {
                    if let Ok(keep) = value.parse::<usize>() {
                        settings.keep_generations = keep;
                    }
                }
                "prune_logs" => settings.prune_logs = flag(&value, settings.prune_logs),
                "keep_logs" => {
                    if let Ok(keep) = value.parse::<usize>() {
                        settings.keep_logs = keep.max(1);
                    }
                }
                "checkout" => {
                    if !value.is_empty() {
                        settings.checkout = Some(value);
                    }
                }
                "leave" => {
                    settings.leave = match value.as_str() {
                        "frame" | "keep" => LeaveScreen::Frame,
                        "clean" | "clear" => LeaveScreen::Clean,
                        _ => settings.leave,
                    }
                }
                // `run` is what the key was called while the card was named Run.
                "show_status" | "show_run" => {
                    settings.show_status = flag(&value, settings.show_status)
                }
                "show_activity" => settings.show_activity = flag(&value, settings.show_activity),
                "show_logs" => settings.show_logs = flag(&value, settings.show_logs),
                "show_downloads" => settings.show_downloads = flag(&value, settings.show_downloads),
                "show_builds" => settings.show_builds = flag(&value, settings.show_builds),
                "show_activity_history" => {
                    settings.show_activity_history = flag(&value, settings.show_activity_history)
                }
                "raw_logs" => settings.raw_logs = flag(&value, settings.raw_logs),
                // A quoted value arrives unquoted; an empty query is still a saved query.
                "log_filter_query" => settings.log_filter_query = Some(value),
                "log_filter_kinds" => settings.log_filter_kinds = Some(value),
                "show_cleanup_progress" => {
                    settings.show_cleanup_progress = flag(&value, settings.show_cleanup_progress)
                }
                _ => {}
            }
        }
        settings
    }

    /// The file a person edits: every setting, with what it does.
    pub fn render(&self) -> String {
        let checkout = match &self.checkout {
            Some(checkout) => format!("checkout = \"{checkout}\""),
            None => "# checkout = \"/home/you/.config/nixos-dotfiles\"".into(),
        };
        let log_filter_query = match &self.log_filter_query {
            Some(query) => format!("log_filter_query = \"{query}\""),
            None => "# log_filter_query = \"\"".into(),
        };
        let log_filter_kinds = match &self.log_filter_kinds {
            Some(kinds) => format!("log_filter_kinds = \"{kinds}\""),
            None => "# log_filter_kinds = \"errors,warnings\"".into(),
        };
        format!(
            "\\
# Settings for nix-rebuild. Every line is optional; a missing one keeps its
# default. Command-line flags win over this file.

# Pruning after a successful rebuild: ask, always or never.
cleanup = \"{}\"
# How many generations cleanup keeps.
keep_generations = {}
# Drop the oldest recorded runs when a run finishes, and how many stay.
prune_logs = {}
keep_logs = {}
# The checkout a run rebuilds when neither --flake nor the machine declares one.
{checkout}

# What the shell's screen is left with when the frame closes. The frame itself is
# drawn on the alternate screen, so it never reaches the scrollback. `clean` leaves
# only a rebuild's closing line under the prompt (nothing for a reopened run);
# `frame` draws the last frame again on the shell's screen.
leave = \"{}\"

# What the dashboard starts with. The Settings dialog writes these.
show_status = {}
show_activity = {}
show_logs = {}
show_downloads = {}
show_builds = {}
show_activity_history = {}
raw_logs = {}
# The Logs filter a start begins with. `Save as default` in the filter dialog writes them.
{log_filter_query}
{log_filter_kinds}
show_cleanup_progress = {}
",
            self.cleanup.stored(),
            self.keep_generations,
            self.prune_logs,
            self.keep_logs,
            self.leave.label(),
            self.show_status,
            self.show_activity,
            self.show_logs,
            self.show_downloads,
            self.show_builds,
            self.show_activity_history,
            self.raw_logs,
            self.show_cleanup_progress,
        )
    }

    /// Where the file lives. `NIX_REBUILD_CONFIG` lets the system scope read the
    /// invoking user's file after re-executing itself under sudo.
    pub fn path() -> PathBuf {
        if let Ok(path) = env::var("NIX_REBUILD_CONFIG") {
            return PathBuf::from(path);
        }
        let base = env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|_| PathBuf::from(".config"));
        base.join("nix-rebuild/config.toml")
    }

    /// Read the file, writing the annotated defaults when there is none yet.
    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(_) => {
                let settings = Self::default();
                let _ = settings.store(&path);
                settings
            }
        }
    }

    /// Write the settings back, ignoring a home that cannot be written.
    pub fn save(&self) {
        let _ = self.store(&Self::path());
    }

    fn store(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.render())
    }
}

/// A boolean that keeps its previous value when the text is not one.
fn flag(value: &str, current: bool) -> bool {
    match value {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" => false,
        _ => current,
    }
}

#[cfg(test)]
mod tests;
