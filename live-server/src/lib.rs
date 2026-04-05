pub mod db;
pub mod grpc;
pub mod parse;
pub mod routes;

use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// A single weather reading scraped from the ARL:UT Lake Travis station.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeatherReading {
    pub id: Option<i64>,
    /// UTC timestamp when we performed the scrape.
    pub scraped_at: chrono::DateTime<chrono::Utc>,
    /// Local time string printed on the station page.
    pub station_time: String,

    // Wind (knots)
    pub wind_speed_kn: f64,
    pub wind_direction: String,
    pub wind_direction_deg: i32,
    pub wind_avg_kn: f64,
    pub wind_hi_kn: f64,
    pub wind_hi_dir_deg: i32,
    pub wind_rms_kn: f64,
    pub wind_vector_avg_kn: f64,
    pub wind_vector_dir_deg: i32,

    // Atmosphere
    pub temperature_f: f64,
    pub humidity_pct: f64,
    pub barometer_inhg: f64,
    pub barometer_trend: Option<f64>,
    pub rain_in: f64,
    pub rain_rate_in_hr: f64,
    pub wind_chill_f: f64,
    pub heat_index_f: f64,
    pub dewpoint_f: f64,
}

pub struct AppState {
    pub db: db::Db,
    pub http: reqwest::Client,
}

const STATION_URL: &str = "https://wwwext.arlut.utexas.edu/weather/lake/";

/// Resolve DB path relative to the current working directory (same as the HTTP server binary).
pub fn resolve_db_path(db_path_arg: &str) -> String {
    let p = std::path::Path::new(db_path_arg);
    if p.is_absolute() {
        db_path_arg.to_string()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(db_path_arg))
            .unwrap_or_else(|_| std::path::PathBuf::from(db_path_arg))
            .to_string_lossy()
            .to_string()
    }
}

/// Scrape the station once and store a row (CLI `live-server pull` / dev bootstrap).
pub async fn pull_once(db_path: &str) -> anyhow::Result<WeatherReading> {
    let db_path = resolve_db_path(db_path);
    let db = db::Db::open(&db_path)?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("live-server/0.1 (pigeonstorm.com)")
        .build()?;
    let state = AppState { db, http };
    scrape_and_store(&state).await
}

pub async fn scrape_loop(state: Arc<AppState>, interval_secs: u64) {
    // First tick fires immediately.
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match scrape_and_store(&state).await {
            Ok(r) => info!(
                wind_kn = r.wind_speed_kn,
                dir = %r.wind_direction,
                "stored reading"
            ),
            Err(e) => error!("scrape failed: {e:#}"),
        }
    }
}

pub async fn scrape_and_store(state: &AppState) -> anyhow::Result<WeatherReading> {
    let html = state
        .http
        .get(STATION_URL)
        .header("Cache-Control", "no-cache, no-store, must-revalidate")
        .header("Pragma", "no-cache")
        .send()
        .await?
        .text()
        .await?;
    let reading = parse::scrape_html(&html)?;
    state.db.insert_reading(&reading)?;
    Ok(reading)
}
