//! The lock files kept before an update or a rollback replaced them, and the Settings row that
//! bounds them.

mod store;

pub use store::{Reason, Restored, SavedLock, current, list, listing, meta, prune, restore, save};

use crate::common::config::SettingEntry;

/// The Settings rows this slice owns: how many kept locks survive the next prune.
pub fn settings_entries() -> Vec<SettingEntry> {
    vec![SettingEntry::number(
        "Locks",
        "Locks kept",
        |settings| settings.keep_locks,
        |settings, delta| {
            let step = if delta < 0 { -1 } else { 1 };
            settings.keep_locks = (settings.keep_locks as i64 + step).clamp(1, 500) as usize;
        },
    )]
}

#[cfg(test)]
mod tests;
