//! Integration tests for the agent pipeline using mock HTTP servers.

use kiteagent_shared::Config;

fn test_config(base_url: &str) -> Config {
    let content = format!(
        r#"
[location]
name = "Windy Point"
lat = 30.4597
lon = -97.9653

[user]
name = "Test"
weight_kg = 84

[gear]
available = ["kitefoil", "twintip", "windfoil"]

[gear.kites]
sizes = [5, 7, 9, 12, 14]

[gear.windfoil_sails]
sizes = [3.5, 4.5, 5.5]

[notification]
method = "webpush"
server_url = "{}"
push_secret = "test-secret"

[server]
bind = "0.0.0.0:8080"
vapid_subject = "mailto:victor@pigeonstorm.com"

[schedule]
fetch_interval_min = 60
morning_digest_hour = 7
opportunity_lookahead_hours = 4
notification_cooldown_hours = 4
max_notifications_per_day = 3

[thresholds]
min_wind_kn = 8.0
max_wind_kn = 40.0
max_gust_ratio = 1.6
min_session_hours = 2
bad_directions_deg = [[0, 90], [315, 360]]

[storage]
db_path = ":memory:"
log_dir = "logs"
log_days = 30
"#,
        base_url
    );
    Config::parse(&content).unwrap()
}

const FIXTURE_GOOD: &str = include_str!("fixtures/open_meteo_good_conditions.json");
const FIXTURE_LIGHT: &str = include_str!("fixtures/open_meteo_light_wind.json");

#[tokio::test]
async fn conditions_evaluate_good_forecast_produces_windows() {
    let content = FIXTURE_GOOD;
    let resp: kiteagent_agent::weather::OpenMeteoResponse = serde_json::from_str(&content).unwrap();
    let forecast = kiteagent_agent::weather::parse_open_meteo(resp);
    let cfg = test_config("http://localhost:8080");
    let windows = kiteagent_agent::conditions::evaluate(&forecast, &cfg);
    assert!(!windows.is_empty());
    assert!(windows[0].disciplines.contains(&"kitefoil".to_string()));
    assert!(windows[0].disciplines.contains(&"twintip".to_string()));
}

#[tokio::test]
async fn conditions_evaluate_light_wind_produces_no_windows() {
    let content = FIXTURE_LIGHT;
    let resp: kiteagent_agent::weather::OpenMeteoResponse = serde_json::from_str(&content).unwrap();
    let forecast = kiteagent_agent::weather::parse_open_meteo(resp);
    let cfg = test_config("http://localhost:8080");
    let windows = kiteagent_agent::conditions::evaluate(&forecast, &cfg);
    assert!(windows.is_empty());
}
