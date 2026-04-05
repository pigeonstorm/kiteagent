use chrono::{Datelike, NaiveDateTime, Timelike};
use kiteagent_shared::Config;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::weather::{Forecast, HourlySlot};

const KITEFOIL_MIN_KN: f64 = 8.0;
const TWINTIP_MIN_KN: f64 = 16.0;
const WINDFOIL_MIN_KN: f64 = 10.0;

const THUNDERSTORM_WMO_MIN: u32 = 95;
const THUNDERSTORM_WMO_MAX: u32 = 99;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RideableWindow {
    pub start: String,
    pub end: String,
    pub avg_kn: f64,
    pub dir_deg: f64,
    pub disciplines: Vec<String>,
}

fn wind_in_bad_direction(dir_deg: f64, bad_directions: &[Vec<f64>]) -> bool {
    for range in bad_directions {
        if range.len() >= 2 {
            let lo = range[0];
            let hi = range[1];
            if lo <= hi {
                if dir_deg >= lo && dir_deg <= hi {
                    return true;
                }
            } else {
                if dir_deg >= lo || dir_deg <= hi {
                    return true;
                }
            }
        }
    }
    false
}

fn viable_disciplines(wind_kn: f64) -> Vec<&'static str> {
    let mut d = Vec::new();
    if wind_kn >= KITEFOIL_MIN_KN {
        d.push("kitefoil");
    }
    if wind_kn >= TWINTIP_MIN_KN {
        d.push("twintip");
    }
    if wind_kn >= WINDFOIL_MIN_KN {
        d.push("windfoil");
    }
    d
}

fn slot_viable(slot: &HourlySlot, cfg: &Config) -> Option<Vec<String>> {
    if slot.wind_speed_kn < KITEFOIL_MIN_KN {
        return None;
    }
    if wind_in_bad_direction(slot.wind_direction_deg, &cfg.thresholds.bad_directions_deg) {
        return None;
    }
    let gust_ratio = if slot.wind_speed_kn > 0.0 {
        slot.wind_gusts_kn / slot.wind_speed_kn
    } else {
        1.0
    };
    if gust_ratio > cfg.thresholds.max_gust_ratio {
        return None;
    }
    if slot.wind_speed_kn > cfg.thresholds.max_wind_kn {
        return None;
    }
    if slot.weather_code >= THUNDERSTORM_WMO_MIN && slot.weather_code <= THUNDERSTORM_WMO_MAX {
        return None;
    }
    let disciplines: Vec<String> = viable_disciplines(slot.wind_speed_kn)
        .into_iter()
        .filter(|d| cfg.gear.available.iter().any(|a| a == *d))
        .map(String::from)
        .collect();
    if disciplines.is_empty() {
        return None;
    }
    Some(disciplines)
}

// ── Daylight helpers ─────────────────────────────────────────────────────────

/// Returns the day-of-month for the Nth occurrence of `weekday` in (year, month).
fn nth_weekday(year: i32, month: u32, weekday: chrono::Weekday, n: u32) -> Option<u32> {
    let mut count = 0u32;
    for d in 1u32..=31 {
        match chrono::NaiveDate::from_ymd_opt(year, month, d) {
            Some(date) if date.weekday() == weekday => {
                count += 1;
                if count == n {
                    return Some(d);
                }
            }
            None => break,
            _ => {}
        }
    }
    None
}

/// True if the date falls within US CDT (2nd Sun Mar → 1st Sun Nov, exclusive).
fn is_cdt(year: i32, month: u32, day: u32) -> bool {
    if month < 3 || month > 11 {
        return false;
    }
    if month > 3 && month < 11 {
        return true;
    }
    if month == 3 {
        let start = nth_weekday(year, 3, chrono::Weekday::Sun, 2).unwrap_or(99);
        return day >= start;
    }
    // November: CDT until (but not including) the 1st Sunday.
    let end = nth_weekday(year, 11, chrono::Weekday::Sun, 1).unwrap_or(1);
    day < end
}

/// Returns (sunrise_utc_hours, sunset_utc_hours) using a Spencer/NOAA approximation
/// accurate to within a few minutes — sufficient for filtering kite sessions.
fn solar_times_utc(lat_deg: f64, lon_deg: f64, year: i32, month: u32, day: u32) -> (f64, f64) {
    use std::f64::consts::PI;
    let doy = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .map(|d| d.ordinal() as f64)
        .unwrap_or(1.0);
    let b = (360.0 / 365.0 * (doy - 81.0)) * (PI / 180.0);
    let declination = 23.45_f64.to_radians() * b.sin();
    let eot_min = 9.87 * (2.0 * b).sin() - 7.53 * b.cos() - 1.5 * b.sin();
    // Solar noon in UTC hours for this longitude.
    let solar_noon_utc = 12.0 - lon_deg / 15.0 - eot_min / 60.0;
    let lat = lat_deg.to_radians();
    let cos_ha = -(lat.tan() * declination.tan());
    if cos_ha <= -1.0 {
        return (0.0, 24.0); // midnight sun
    }
    if cos_ha >= 1.0 {
        return (12.0, 12.0); // polar night
    }
    let ha_hours = cos_ha.acos().to_degrees() / 15.0;
    (solar_noon_utc - ha_hours, solar_noon_utc + ha_hours)
}

/// Returns true when `time_str` (Open-Meteo local time, e.g. "2026-02-24T14:00") falls
/// between sunrise and sunset at the given coordinates.
fn is_daylight(time_str: &str, lat_deg: f64, lon_deg: f64) -> bool {
    let dt = match NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M") {
        Ok(d) => d,
        Err(_) => return true, // parse failure → don't filter
    };
    // Convert local (America/Chicago) to UTC hours.
    let utc_offset = if is_cdt(dt.year(), dt.month(), dt.day()) {
        -5.0_f64
    } else {
        -6.0_f64
    };
    let slot_utc = dt.hour() as f64 + dt.minute() as f64 / 60.0 - utc_offset;
    let (sunrise, sunset) = solar_times_utc(lat_deg, lon_deg, dt.year(), dt.month(), dt.day());
    slot_utc >= sunrise && slot_utc <= sunset
}

// ── Window evaluation ─────────────────────────────────────────────────────────

pub fn evaluate(forecast: &Forecast, cfg: &Config) -> Vec<RideableWindow> {
    let mut windows: Vec<RideableWindow> = Vec::new();
    let mut i = 0;
    let min_hours = cfg.thresholds.min_session_hours as usize;

    let lat = cfg.location.lat;
    let lon = cfg.location.lon;

    while i < forecast.slots.len() {
        // Skip dark hours before even trying to open a window.
        if cfg.thresholds.daylight_only && !is_daylight(&forecast.slots[i].time, lat, lon) {
            i += 1;
            continue;
        }

        let run_start = i;
        let mut run_disciplines: Vec<String> = Vec::new();
        let mut run_wind_sum = 0.0;
        let mut run_count = 0;

        while i < forecast.slots.len() {
            // A dark slot always breaks the window (can't kite after sunset).
            if cfg.thresholds.daylight_only && !is_daylight(&forecast.slots[i].time, lat, lon) {
                break;
            }
            if let Some(disc) = slot_viable(&forecast.slots[i], cfg) {
                run_count += 1;
                run_wind_sum += forecast.slots[i].wind_speed_kn;
                for d in &disc {
                    if !run_disciplines.contains(d) {
                        run_disciplines.push(d.clone());
                    }
                }
                i += 1;
            } else {
                break;
            }
        }

        if run_count >= min_hours && !run_disciplines.is_empty() {
            let avg_kn = run_wind_sum / run_count as f64;
            let dir_deg = forecast.slots[run_start].wind_direction_deg;
            windows.push(RideableWindow {
                start: forecast.slots[run_start].time.clone(),
                end: forecast.slots[i - 1].time.clone(),
                avg_kn,
                dir_deg,
                disciplines: run_disciplines,
            });
        }

        if i < forecast.slots.len() && !slot_viable(&forecast.slots[i], cfg).is_some() {
            i += 1;
        }
    }

    info!(windows_found = windows.len(), "analysis complete");
    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiteagent_shared::{
        GearConfig, KiteSizes, LocationConfig, NotificationConfig, SailSizes, ScheduleConfig,
        ServerConfig, StorageConfig, ThresholdsConfig, UserConfig,
    };

    fn test_config() -> Config {
        Config {
            location: LocationConfig {
                name: "Test".into(),
                lat: 30.0,
                lon: -98.0,
            },
            user: UserConfig {
                name: "Test".into(),
                weight_kg: 84,
            },
            gear: GearConfig {
                available: vec!["kitefoil".into(), "twintip".into(), "windfoil".into()],
                kites: KiteSizes {
                    sizes: vec![5.0, 7.0, 9.0, 12.0, 14.0],
                },
                windfoil_sails: SailSizes {
                    sizes: vec![3.5, 4.5, 5.5],
                },
            },
            notification: NotificationConfig {
                method: "webpush".into(),
                server_url: "http://localhost:8080".into(),
                push_secret: "secret".into(),
            },
            server: ServerConfig {
                bind: "0.0.0.0:8080".into(),
                vapid_subject: "mailto:victor@pigeonstorm.com".into(),
                hrrr_url: None,
                public_base_url: None,
            },
            schedule: ScheduleConfig {
                fetch_interval_min: 60,
                morning_digest_hour: 7,
                opportunity_lookahead_hours: 4,
                notification_cooldown_hours: 4,
                max_notifications_per_day: 3,
            },
            thresholds: ThresholdsConfig {
                min_wind_kn: 8.0,
                max_wind_kn: 40.0,
                max_gust_ratio: 1.6,
                min_session_hours: 2,
                bad_directions_deg: vec![vec![0.0, 90.0], vec![315.0, 360.0]],
                daylight_only: false, // tests use fixed timestamps; disable solar filter
            },
            storage: StorageConfig {
                db_path: ":memory:".into(),
                log_dir: "logs".into(),
                log_days: 30,
            },
            live: None,
        }
    }

    fn slot(time: &str, wind: f64, gusts: f64, dir: f64, wmo: u32) -> HourlySlot {
        HourlySlot {
            time: time.to_string(),
            wind_speed_kn: wind,
            wind_direction_deg: dir,
            wind_gusts_kn: gusts,
            temperature_c: Some(25.0),
            weather_code: wmo,
        }
    }

    #[test]
    fn no_windows_when_wind_below_minimum() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 5.0, 8.0, 200.0, 0),
                slot("2026-02-24T13:00", 6.0, 9.0, 200.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert!(w.is_empty());
    }

    #[test]
    fn foil_window_8_to_15_kn() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 10.0, 14.0, 200.0, 0),
                slot("2026-02-24T13:00", 12.0, 16.0, 210.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert_eq!(w.len(), 1);
        assert!(w[0].disciplines.contains(&"kitefoil".to_string()));
        assert!(!w[0].disciplines.contains(&"twintip".to_string()));
    }

    #[test]
    fn twintip_requires_16_kn_minimum() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 24.0, 200.0, 0),
                slot("2026-02-24T13:00", 20.0, 26.0, 210.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert_eq!(w.len(), 1);
        assert!(w[0].disciplines.contains(&"twintip".to_string()));
    }

    #[test]
    fn bad_direction_north_rejected() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 24.0, 45.0, 0),
                slot("2026-02-24T13:00", 18.0, 24.0, 50.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert!(w.is_empty());
    }

    #[test]
    fn high_gust_ratio_rejected() {
        let mut cfg = test_config();
        cfg.thresholds.max_gust_ratio = 1.5;
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 10.0, 20.0, 200.0, 0),
                slot("2026-02-24T13:00", 10.0, 20.0, 200.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert!(w.is_empty());
    }

    #[test]
    fn thunderstorm_wmo_code_rejected() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 24.0, 200.0, 95),
                slot("2026-02-24T13:00", 18.0, 24.0, 200.0, 95),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert!(w.is_empty());
    }

    #[test]
    fn consecutive_hours_grouped_into_single_window() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 22.0, 200.0, 0),
                slot("2026-02-24T13:00", 19.0, 23.0, 210.0, 0),
                slot("2026-02-24T14:00", 20.0, 24.0, 205.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].start, "2026-02-24T12:00");
        assert_eq!(w[0].end, "2026-02-24T14:00");
    }

    #[test]
    fn non_consecutive_hours_become_two_windows() {
        let cfg = test_config();
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 22.0, 200.0, 0),
                slot("2026-02-24T13:00", 19.0, 23.0, 210.0, 0),
                slot("2026-02-24T14:00", 5.0, 8.0, 200.0, 0),
                slot("2026-02-24T15:00", 18.0, 22.0, 200.0, 0),
                slot("2026-02-24T16:00", 19.0, 23.0, 200.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].end, "2026-02-24T13:00");
        assert_eq!(w[1].start, "2026-02-24T15:00");
    }

    #[test]
    fn min_session_hours_gate_filters_short_window() {
        let mut cfg = test_config();
        cfg.thresholds.min_session_hours = 3;
        let forecast = Forecast {
            source: "test".into(),
            slots: vec![
                slot("2026-02-24T12:00", 18.0, 22.0, 200.0, 0),
                slot("2026-02-24T13:00", 18.0, 22.0, 200.0, 0),
            ],
        };
        let w = evaluate(&forecast, &cfg);
        assert!(w.is_empty());
    }
}
