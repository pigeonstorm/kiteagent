//! Parsing forecast window timestamps and scheduling helpers.

use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use chrono_tz::America::Chicago;

/// Parse a forecast window start into UTC.
///
/// HRRR / Open-Meteo responses use `timezone=America/Chicago` and return local wall times **without**
/// a `Z` suffix (e.g. `2026-04-11T14:00`). RFC3339-only parsing fails on those strings, which used to
/// block all opportunity notifications.
pub fn parse_window_start_utc(start: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(start) {
        return Some(dt.with_timezone(&Utc));
    }
    let naive = NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M").ok())?;
    Chicago
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn window_starts_within_hours(start: &str, hours: i64, now: &DateTime<Utc>) -> bool {
    let Some(start_dt) = parse_window_start_utc(start) else {
        tracing::warn!(%start, "failed to parse window start time");
        return false;
    };
    let diff = start_dt - *now;
    diff >= Duration::hours(0) && diff <= Duration::hours(hours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chicago_local_without_zone() {
        let dt = parse_window_start_utc("2026-06-15T14:00").expect("parse");
        let chicago = dt.with_timezone(&Chicago);
        assert_eq!(chicago.format("%Y-%m-%d %H:%M").to_string(), "2026-06-15 14:00");
    }

    #[test]
    fn parses_rfc3339_z() {
        let dt = parse_window_start_utc("2026-06-15T19:00:00Z").expect("parse");
        assert_eq!(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2026-06-15T19:00:00Z");
    }

    #[test]
    fn window_within_hours_respects_chicago_local() {
        // 14:00 Chicago = 19:00 UTC on this date (CDT, UTC-5)
        let now = Chicago
            .with_ymd_and_hms(2026, 6, 15, 13, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(window_starts_within_hours("2026-06-15T14:00", 4, &now));
        assert!(!window_starts_within_hours("2026-06-15T18:00", 4, &now));
    }
}
