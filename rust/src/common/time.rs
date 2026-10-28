//! Local-time stamps for run ids and date columns: one epoch-to-calendar conversion.

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Timelike};

/// A local-time `YYYYMMDD-HHMMSS` stamp, cut from the same calendar fields the date
/// column reads, so the two cannot disagree.
pub fn backup_stamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    stamp_at(seconds)
}

/// The same stamp for an epoch second, split out so tests can check known instants.
pub fn stamp_at(epoch_seconds: i64) -> String {
    let (year, month, day, hour, minute, second) = local_parts(epoch_seconds);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// The local calendar fields of an epoch second: year, month, day, hour, minute, second.
/// One conversion, so the id stamp and the date column cannot disagree.
pub fn local_parts(epoch_seconds: i64) -> (i64, i64, i64, i64, i64, i64) {
    let clock = local_clock(epoch_seconds);
    (
        clock.year() as i64,
        clock.month() as i64,
        clock.day() as i64,
        clock.hour() as i64,
        clock.minute() as i64,
        clock.second() as i64,
    )
}

/// The wall-clock of an epoch second on the local offset, so a run across a DST
/// boundary stamps the time the user saw. An instant the local zone refuses is read as UTC.
fn local_clock(epoch_seconds: i64) -> NaiveDateTime {
    Local
        .timestamp_opt(epoch_seconds, 0)
        .single()
        .map(|local| local.naive_local())
        .or_else(|| DateTime::from_timestamp(epoch_seconds, 0).map(|utc| utc.naive_utc()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_stamp_calendar_arithmetic_matches_known_dates() {
        use super::local_parts;

        // Pinned to UTC: `day * 86_400` is midnight UTC, and the function reads the
        // local zone, so the date it returns is the machine's calendar day.
        temp_env::with_var("TZ", Some("UTC"), || {
            let date = |day: i64| {
                let (year, month, day_of_month, _, _, _) = local_parts(day * 86_400);
                (year, month, day_of_month)
            };
            assert_eq!(date(0), (1970, 1, 1), "the epoch");
            assert_eq!(date(-1), (1969, 12, 31), "before the epoch");
            assert_eq!(date(19_723), (2024, 1, 1), "a leap year");
            assert_eq!(date(20_716), (2026, 9, 20), "a plain year");
            assert_eq!(date(11_016), (2000, 2, 29), "a leap day");
        });
    }

    #[test]
    fn backup_stamp_is_a_sortable_local_timestamp() {
        let stamp = super::stamp_at(1_774_000_000);
        assert_eq!(stamp.len(), 15, "YYYYMMDD-HHMMSS, got {stamp}");
        assert_eq!(&stamp[8..9], "-", "date and time are separated");
        assert!(
            stamp.chars().filter(char::is_ascii_digit).count() == 14,
            "only the separator is punctuation, got {stamp}",
        );

        let later = super::stamp_at(1_774_000_001);
        assert!(later >= stamp, "stamps are sortable as text");
    }
}
