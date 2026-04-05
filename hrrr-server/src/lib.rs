pub mod db;
pub mod hrrr;
pub mod rate_limit;
pub mod routes;

use tracing::error;

pub struct AppState {
    pub db: db::Db,
    pub http: reqwest::Client,
}

/// Windy Point default (matches POST `/pull` query defaults in `routes`).
pub const DEFAULT_PULL_LAT: f64 = 30.46;
pub const DEFAULT_PULL_LON: f64 = -97.97;
pub const DEFAULT_FORECAST_DAYS: u32 = 2;

/// Fetch HRRR from NOMADS and upsert Open-Meteo-shaped JSON into the forecast cache.
/// Same work as an admin POST `/pull`; used for CLI bootstrap (`hrrr-server pull`).
pub async fn pull_forecast_cache(
    http: &reqwest::Client,
    db: &db::Db,
    lat: f64,
    lon: f64,
    forecast_days: u32,
) -> anyhow::Result<(hrrr::HrrrRun, usize, String)> {
    let lat = (lat * 100.0).round() / 100.0;
    let lon = (lon * 100.0).round() / 100.0;

    let (run, slots) = hrrr::fetch_hrrr(http, lat, lon, forecast_days).await?;
    let json_val = hrrr::to_openmeteo_json(&slots, "kn");
    let raw_json = serde_json::to_string(&json_val).unwrap_or_default();
    let valid_from = slots.first().map(|s| s.time.as_str()).unwrap_or("");
    let valid_to = slots.last().map(|s| s.time.as_str()).unwrap_or("");
    let fetched_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    if let Err(e) = db.upsert_forecast_cache(
        lat,
        lon,
        &run.date,
        run.cycle as i32,
        &fetched_at,
        valid_from,
        valid_to,
        &raw_json,
    ) {
        error!(error = %e, "failed to cache forecast after pull");
        return Err(e);
    }

    Ok((run, slots.len(), fetched_at))
}
