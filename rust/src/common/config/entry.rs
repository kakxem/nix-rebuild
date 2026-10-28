//! One row a slice contributes to the Settings card, so no option list lives in the card.

use crate::common::actions::Command;

use super::Settings;

/// A figure the card prints from the snapshot the runtime hands it, not from the file.
/// The card owns the snapshot, so an entry never names the slice that measures it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// How many runs the journal holds.
    RecordedRuns,
    /// How many bytes those runs take.
    RecordedBytes,
}

/// How a row reads and changes its setting.
#[derive(Debug, Clone, Copy)]
pub enum SettingKind {
    /// A row that is a button: its command runs and the file is left alone.
    Action { command: Command },
    /// A figure that is read, never changed: the arrows pass over it.
    Reading { reading: Reading },
    Toggle {
        read: fn(&Settings) -> bool,
        flip: fn(&mut Settings),
    },
    Choice {
        read: fn(&Settings) -> String,
        cycle: fn(&mut Settings),
    },
    /// A number the card draws with `-` and `+` controls at the end of the row.
    Number {
        read: fn(&Settings) -> usize,
        step: fn(&mut Settings, i64),
    },
}

/// A setting a slice owns: its section, its label, and how to read and change it.
#[derive(Debug, Clone, Copy)]
pub struct SettingEntry {
    pub section: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
}

impl SettingEntry {
    pub fn action(section: &'static str, label: &'static str, command: Command) -> Self {
        Self {
            section,
            label,
            kind: SettingKind::Action { command },
        }
    }

    /// A row that prints a figure the card is handed instead of a value from the file.
    pub fn figure(section: &'static str, label: &'static str, reading: Reading) -> Self {
        Self {
            section,
            label,
            kind: SettingKind::Reading { reading },
        }
    }

    pub fn toggle(
        section: &'static str,
        label: &'static str,
        read: fn(&Settings) -> bool,
        flip: fn(&mut Settings),
    ) -> Self {
        Self {
            section,
            label,
            kind: SettingKind::Toggle { read, flip },
        }
    }

    pub fn choice(
        section: &'static str,
        label: &'static str,
        read: fn(&Settings) -> String,
        cycle: fn(&mut Settings),
    ) -> Self {
        Self {
            section,
            label,
            kind: SettingKind::Choice { read, cycle },
        }
    }

    pub fn number(
        section: &'static str,
        label: &'static str,
        read: fn(&Settings) -> usize,
        step: fn(&mut Settings, i64),
    ) -> Self {
        Self {
            section,
            label,
            kind: SettingKind::Number { read, step },
        }
    }

    pub fn shown(&self, settings: &Settings) -> String {
        match self.kind {
            SettingKind::Action { .. } => String::new(),
            // The card formats this one from the snapshot it holds, not from the file.
            SettingKind::Reading { .. } => String::new(),
            SettingKind::Toggle { read, .. } => {
                if read(settings) {
                    "on".into()
                } else {
                    "off".into()
                }
            }
            SettingKind::Choice { read, .. } => read(settings),
            SettingKind::Number { read, .. } => read(settings).to_string(),
        }
    }

    pub fn apply(&self, settings: &mut Settings, delta: i64) {
        match self.kind {
            SettingKind::Action { .. } | SettingKind::Reading { .. } => {}
            SettingKind::Toggle { flip, .. } => flip(settings),
            SettingKind::Choice { cycle, .. } => cycle(settings),
            SettingKind::Number { step, .. } => step(settings, delta),
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self.kind, SettingKind::Number { .. })
    }

    /// Whether the arrows may land on the row: a figure they cannot change is skipped.
    pub fn is_selectable(&self) -> bool {
        !matches!(self.kind, SettingKind::Reading { .. })
    }

    /// The figure a read-only row prints.
    pub fn reading(&self) -> Option<Reading> {
        match self.kind {
            SettingKind::Reading { reading } => Some(reading),
            _ => None,
        }
    }

    /// The command a button row carries out.
    pub fn action_command(&self) -> Option<Command> {
        match self.kind {
            SettingKind::Action { command } => Some(command),
            _ => None,
        }
    }

    /// A toggle's state, so the card can colour it; `None` for a row that shows a value.
    pub fn is_on(&self, settings: &Settings) -> Option<bool> {
        match self.kind {
            SettingKind::Toggle { read, .. } => Some(read(settings)),
            _ => None,
        }
    }
}
