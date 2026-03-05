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

    // Wind
    pub wind_speed_mph: f64,
    pub wind_direction: String,
    pub wind_direction_deg: i32,
    pub wind_avg_mph: f64,
    pub wind_hi_mph: f64,
    pub wind_hi_dir_deg: i32,
    pub wind_rms_mph: f64,
    pub wind_vector_avg_mph: f64,
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

pub async fn scrape_loop(state: Arc<AppState>, interval_secs: u64) {
    // First tick fires immediately.
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match scrape_and_store(&state).await {
            Ok(r) => info!(
                wind_mph = r.wind_speed_mph,
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
        .send()
        .await?
        .text()
        .await?;
    let reading = parse::scrape_html(&html)?;
    state.db.insert_reading(&reading)?;
    Ok(reading)
}
