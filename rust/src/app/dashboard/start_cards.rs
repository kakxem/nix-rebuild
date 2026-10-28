//! The "Panels" section of the Settings dialog: which cards a run starts with.
//! It lives here, not in a slice, because the cards are the frame's own.

use crate::common::config::SettingEntry;

pub fn entries() -> Vec<SettingEntry> {
    vec![
        SettingEntry::toggle(
            "Panels",
            "'Status' panel at start",
            |settings| settings.show_status,
            |settings| settings.show_status = !settings.show_status,
        ),
        SettingEntry::toggle(
            "Panels",
            "'Activity' panel at start",
            |settings| settings.show_activity,
            |settings| settings.show_activity = !settings.show_activity,
        ),
        SettingEntry::toggle(
            "Panels",
            "'Logs' panel at start",
            |settings| settings.show_logs,
            |settings| settings.show_logs = !settings.show_logs,
        ),
    ]
}
