use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use crate::hrrr;
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(serve_dashboard))
        .route("/metrics.json", get(metrics))
        .route("/forecast", get(forecast))
        .route("/pull", post(pull))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn is_admin(params: &HashMap<String, String>) -> bool {
    params
        .get("user")
        .map(|u| u.trim().to_lowercase() == "victor")
        .unwrap_or(false)
}

// ── GET /forecast ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[allow(dead_code)]
struct ForecastQuery {
    latitude: f64,
    longitude: f64,
    #[serde(default = "default_hourly")]
    hourly: String,
    #[serde(default = "default_wind_unit")]
    wind_speed_unit: String,
    #[serde(default = "default_forecast_days")]
    forecast_days: u32,
    #[serde(default)]
    timezone: String,
}

fn default_hourly() -> String {
    "windspeed_10m,winddirection_10m,windgusts_10m,temperature_2m,weathercode".to_string()
}
fn default_wind_unit() -> String { "kn".to_string() }
fn default_forecast_days() -> u32 { 2 }

fn round_2dp(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

async fn forecast(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<ForecastQuery>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();
    let ip = addr.ip().to_string();
    let lat = round_2dp(q.latitude);
    let lon = round_2dp(q.longitude);

    let run = hrrr::select_run(q.forecast_days);
    let run_date = &run.date;
    let run_cycle = run.cycle as i32;

    if let Ok(Some(cached)) = state.db.get_cached_forecast(lat, lon, run_date, run_cycle) {
        let duration = start.elapsed().as_millis() as u64;
        let _ = state.db.log_request(&ip, "/forecast", Some(lat), Some(lon), true, 200, duration);
        info!(lat, lon, run_date, run_cycle, cache = "hit", duration_ms = duration, "forecast served");
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            cached.raw_json,
        ).into_response();
    }

    match hrrr::fetch_hrrr(&state.http, lat, lon, q.forecast_days).await {
        Ok((run, slots)) => {
            let json_val = hrrr::to_openmeteo_json(&slots, &q.wind_speed_unit);
            let raw_json = serde_json::to_string(&json_val).unwrap_or_default();

            let valid_from = slots.first().map(|s| s.time.as_str()).unwrap_or("");
            let valid_to = slots.last().map(|s| s.time.as_str()).unwrap_or("");
            let fetched_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            if let Err(e) = state.db.upsert_forecast_cache(
                lat, lon, &run.date, run.cycle as i32,
                &fetched_at, valid_from, valid_to, &raw_json,
            ) {
                error!(error = %e, "failed to cache forecast");
            }

            let duration = start.elapsed().as_millis() as u64;
            let _ = state.db.log_request(&ip, "/forecast", Some(lat), Some(lon), false, 200, duration);
            info!(lat, lon, run_date = %run.date, run_cycle = run.cycle, hours = slots.len(), cache = "miss", duration_ms = duration, "forecast fetched from NOMADS");

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                raw_json,
            ).into_response()
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as u64;
            let _ = state.db.log_request(&ip, "/forecast", Some(lat), Some(lon), false, 502, duration);
            let _ = state.db.log_error("nomads_fetch", &e.to_string());
            error!(error = %e, "HRRR fetch failed");

            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            ).into_response()
        }
    }
}

// ── GET /metrics.json ───────────────────────────────────────────────────

#[derive(Serialize)]
struct MetricsResponse {
    last_fetch: Option<LastFetchInfo>,
    cache: CacheInfo,
    requests: RequestsInfo,
    rate_limited_last_24h: i64,
    errors_last_24h: i64,
    top_callers: Vec<CallerInfo>,
    recent_errors: Vec<crate::db::ErrorEntry>,
}

#[derive(Serialize)]
struct LastFetchInfo {
    at: String,
    valid_from: String,
    valid_to: String,
}

#[derive(Serialize)]
struct CacheInfo {
    entries: i64,
    hit_rate_pct: f64,
}

#[derive(Serialize)]
struct RequestsInfo {
    last_24h: i64,
    last_1h: i64,
    by_hour: Vec<i64>,
}

#[derive(Serialize)]
struct CallerInfo {
    ip: String,
    requests_24h: i64,
}

async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let last_fetch = state.db.last_cache_entry().ok().flatten().map(|c| LastFetchInfo {
        at: c.fetched_at,
        valid_from: c.valid_from,
        valid_to: c.valid_to,
    });

    let entries = state.db.cache_entry_count().unwrap_or(0);
    let hit_rate = state.db.cache_hit_rate_24h().unwrap_or(0.0);

    let last_24h = state.db.requests_last_24h().unwrap_or(0);
    let last_1h = state.db.requests_last_1h().unwrap_or(0);
    let by_hour = state.db.requests_by_hour_24h().unwrap_or_else(|_| vec![0; 24]);

    let rate_limited = state.db.rate_limited_last_24h().unwrap_or(0);
    let errors = state.db.errors_last_24h().unwrap_or(0);

    let top_callers: Vec<CallerInfo> = state
        .db
        .top_callers_24h(10)
        .unwrap_or_default()
        .into_iter()
        .map(|(ip, cnt)| CallerInfo { ip, requests_24h: cnt })
        .collect();

    let recent_errors = state.db.recent_errors(5).unwrap_or_default();

    Json(MetricsResponse {
        last_fetch,
        cache: CacheInfo {
            entries,
            hit_rate_pct: (hit_rate * 10.0).round() / 10.0,
        },
        requests: RequestsInfo {
            last_24h,
            last_1h,
            by_hour,
        },
        rate_limited_last_24h: rate_limited,
        errors_last_24h: errors,
        top_callers,
        recent_errors,
    })
}

// ── POST /pull (admin only) ──────────────────────────────────────────────

#[derive(Deserialize)]
struct PullQuery {
    user: Option<String>,
    #[serde(default = "default_pull_lat")]
    latitude: f64,
    #[serde(default = "default_pull_lon")]
    longitude: f64,
    #[serde(default = "default_forecast_days")]
    forecast_days: u32,
}

fn default_pull_lat() -> f64 { 30.46 }
fn default_pull_lon() -> f64 { -97.97 }

async fn pull(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PullQuery>,
) -> impl IntoResponse {
    let mut params = HashMap::new();
    if let Some(u) = &q.user {
        params.insert("user".to_string(), u.clone());
    }
    if !is_admin(&params) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "forbidden"})),
        ).into_response();
    }

    let lat = round_2dp(q.latitude);
    let lon = round_2dp(q.longitude);

    info!(lat, lon, forecast_days = q.forecast_days, "admin pull requested");

    match crate::pull_forecast_cache(&state.http, &state.db, lat, lon, q.forecast_days).await {
        Ok((run, hours, fetched_at)) => {
            info!(run_date = %run.date, run_cycle = run.cycle, hours, "admin pull succeeded");
            Json(serde_json::json!({
                "ok": true,
                "fetched_at": fetched_at,
                "run": format!("t{:02}z", run.cycle),
                "hours": hours,
            })).into_response()
        }
        Err(e) => {
            let _ = state.db.log_error("nomads_fetch", &format!("admin pull: {e}"));
            error!(error = %e, "admin pull failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"ok": false, "error": e.to_string()})),
            ).into_response()
        }
    }
}

// ── GET / (dashboard) ───────────────────────────────────────────────────

async fn serve_dashboard(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let admin = is_admin(&params);
    let html = include_str!("../static/index.html")
        .replace("__IS_ADMIN__", if admin { "true" } else { "false" });
    (
        [
            (header::CONTENT_TYPE, "text/html"),
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
        ],
        html,
    )
}
