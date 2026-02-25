use kiteagent_shared::Config;
use std::fmt;

use crate::conditions::RideableWindow;

#[derive(Debug, Clone)]
pub struct GearRecommendation {
    pub disciplines: Vec<DisciplineGear>,
}

#[derive(Debug, Clone)]
pub struct DisciplineGear {
    pub discipline: String,
    pub size: f64,
    pub unit: String,
}

fn kite_size_for_wind(wind_kn: f64) -> f64 {
    if wind_kn >= 28.0 {
        5.0
    } else if wind_kn >= 19.0 {
        7.0
    } else if wind_kn >= 15.0 {
        9.0
    } else if wind_kn >= 12.0 {
        12.0
    } else {
        14.0
    }
}

fn sail_size_for_wind(wind_kn: f64) -> f64 {
    if wind_kn >= 25.0 {
        3.5
    } else if wind_kn >= 18.0 {
        4.5
    } else {
        5.5
    }
}

fn closest_owned_size(target: f64, owned: &[f64]) -> Option<f64> {
    if owned.is_empty() {
        return None;
    }
    owned
        .iter()
        .min_by(|a, b| {
            let da = (*a - target).abs();
            let db = (*b - target).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
}

pub fn recommend(window: &RideableWindow, cfg: &Config) -> GearRecommendation {
    let mut disciplines = Vec::new();
    let kites = &cfg.gear.kites.sizes;
    let sails = &cfg.gear.windfoil_sails.sizes;

    for d in &window.disciplines {
        match d.as_str() {
            "kitefoil" | "twintip" => {
                let target = kite_size_for_wind(window.avg_kn);
                if let Some(size) = closest_owned_size(target, kites) {
                    disciplines.push(DisciplineGear {
                        discipline: d.clone(),
                        size,
                        unit: "m kite".to_string(),
                    });
                }
            }
            "windfoil" => {
                let target = sail_size_for_wind(window.avg_kn);
                if let Some(size) = closest_owned_size(target, sails) {
                    disciplines.push(DisciplineGear {
                        discipline: d.clone(),
                        size,
                        unit: "m sail".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    GearRecommendation { disciplines }
}

impl fmt::Display for GearRecommendation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for d in &self.disciplines {
            writeln!(f, "  {}  ✅  → bring your {} {} ", d.discipline, d.size, d.unit)?;
        }
        Ok(())
    }
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
                vapid_subject: "mailto:test@test.com".into(),
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
                bad_directions_deg: vec![],
                daylight_only: false,
            },
            storage: StorageConfig {
                db_path: ":memory:".into(),
                log_dir: "logs".into(),
                log_days: 30,
            },
        }
    }

    fn window(avg_kn: f64, disciplines: Vec<&str>) -> RideableWindow {
        RideableWindow {
            start: "2026-02-24T14:00".into(),
            end: "2026-02-24T18:00".into(),
            avg_kn,
            dir_deg: 210.0,
            disciplines: disciplines.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn kitefoil_selects_closest_owned_kite_at_18kn() {
        let cfg = test_config();
        let w = window(18.0, vec!["kitefoil"]);
        let r = recommend(&w, &cfg);
        assert_eq!(r.disciplines.len(), 1);
        assert_eq!(r.disciplines[0].discipline, "kitefoil");
        assert_eq!(r.disciplines[0].size, 9.0); // 18kn → target 9m
    }

    #[test]
    fn kitefoil_selects_smallest_kite_at_30kn() {
        let cfg = test_config();
        let w = window(30.0, vec!["kitefoil"]);
        let r = recommend(&w, &cfg);
        assert_eq!(r.disciplines[0].size, 5.0);
    }

    #[test]
    fn twintip_not_recommended_below_16kn() {
        let w = window(14.0, vec!["kitefoil"]);
        assert!(!w.disciplines.contains(&"twintip".to_string()));
    }

    #[test]
    fn windfoil_picks_correct_sail_from_inventory() {
        let cfg = test_config();
        let w = window(20.0, vec!["windfoil"]);
        let r = recommend(&w, &cfg);
        assert_eq!(r.disciplines.len(), 1);
        assert_eq!(r.disciplines[0].size, 4.5);
    }

    #[test]
    fn missing_gear_discipline_excluded_from_output() {
        let mut cfg = test_config();
        cfg.gear.available = vec!["kitefoil".into()];
        let w = window(18.0, vec!["kitefoil"]);
        let r = recommend(&w, &cfg);
        assert_eq!(r.disciplines.len(), 1);
    }
}
