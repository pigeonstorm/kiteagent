use axum::{
    extract::{Query, Request, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use kiteagent_shared::{Config, Db};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::vapid::{send_push, VapidKeys};

pub struct AppState {
    pub db: Db,
    pub vapid: VapidKeys,
    pub push_secret: String,
    pub config: Config,
    pub http: reqwest::Client,
    pub web_push: web_push::WebPushClient,
}

#[derive(Deserialize)]
struct SubscribePayload {
    endpoint: String,
    keys: SubscribeKeys,
}

#[derive(Deserialize)]
struct SubscribeKeys {
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
struct PushPayload {
    title: String,
    body: String,
}

#[derive(Serialize)]
struct StatusResponse {
    service: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    last_forecast_fetch: Option<ForecastFetchInfo>,
    last_analysis: Option<AnalysisInfo>,
    last_notification_sent: Option<NotificationInfo>,
    errors_last_24h: i64,
    subscribers: i64,
}

#[derive(Serialize)]
struct ForecastFetchInfo {
    at: String,
    source: String,
    ok: bool,
}

#[derive(Serialize)]
struct AnalysisInfo {
    at: String,
    windows_found: i32,
    next_window: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct NotificationInfo {
    at: String,
    window_start: String,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/sw.js", get(serve_sw))
        .route("/manifest.json", get(serve_manifest))
        .route("/logo.png", get(serve_logo))
        .route("/subscribe", post(subscribe))
        .route("/push", post(push))
        .route("/test-push", post(test_push))
        .route("/status", get(status))
        .route("/forecast", get(forecast))
        .route("/pull", post(pull_forecast))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn load_index_html() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidates = [
        format!("{}/static/index.html", manifest_dir),
        "server/static/index.html".to_string(),
        "static/index.html".to_string(),
    ];
    for path in &candidates {
        if let Ok(html) = std::fs::read_to_string(path) {
            tracing::debug!(path = %path, "serving index from file");
            return html;
        }
    }
    tracing::debug!("serving embedded index");
    include_str!("../static/index.html").to_string()
}

async fn serve_index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let public_key = state.vapid.public_key_base64url().unwrap_or_default();
    let html = load_index_html().replace("__VAPID_PUBLIC_KEY__", &public_key);
    let headers = [
        (header::CONTENT_TYPE, "text/html"),
        (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
        (header::PRAGMA, "no-cache"),
    ];
    (headers, html)
}

async fn serve_sw() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../static/sw.js"),
    )
}

async fn serve_manifest() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        include_str!("../static/manifest.json"),
    )
}

async fn serve_logo() -> impl IntoResponse {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidates = [
        format!("{}/static/logo.png", manifest_dir),
        "server/static/logo.png".to_string(),
        "static/logo.png".to_string(),
    ];
    
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            return axum::response::Response::builder()
                .header(header::CONTENT_TYPE, "image/png")
                .body(axum::body::Body::from(data))
                .unwrap()
                .into_response();
        }
    }
    
    (StatusCode::NOT_FOUND, "logo not found").into_response()
}

async fn subscribe(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SubscribePayload>,
) -> impl IntoResponse {
    if let Err(e) = state.db.insert_push_subscription(
        &payload.endpoint,
        &payload.keys.p256dh,
        &payload.keys.auth,
    ) {
        tracing::error!(%e, "failed to save subscription");
        return (StatusCode::INTERNAL_SERVER_ERROR, "failed to save subscription").into_response();
    }

    let welcome = serde_json::json!({
        "title": "KiteAgent — You're subscribed! 🪁",
        "body": "You'll receive alerts when wind conditions are good for kiting at Windy Point, Lake Travis.",
    })
    .to_string();
    if let Err(e) =
        send_push(&payload.endpoint, &payload.keys.p256dh, &payload.keys.auth, &welcome, &state.vapid, &state.web_push).await
    {
        tracing::warn!(%e, "failed to send welcome notification");
    }

    (StatusCode::OK, "OK").into_response()
}

async fn test_push(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SubscribePayload>,
) -> impl IntoResponse {
    let body = serde_json::json!({
        "title": "KiteAgent — Test Notification 🪁",
        "body": "Your notification system is working correctly!",
    })
    .to_string();
    match send_push(&payload.endpoint, &payload.keys.p256dh, &payload.keys.auth, &body, &state.vapid, &state.web_push).await {
        Ok(()) => (StatusCode::OK, "OK").into_response(),
        Err(e) => {
            tracing::error!(%e, "test push failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "push failed").into_response()
        }
    }
}

async fn push(State(state): State<Arc<AppState>>, req: Request) -> impl IntoResponse {
    let auth = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    if auth != Some(state.push_secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad body").into_response(),
    };
    let payload: PushPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid JSON").into_response(),
    };

    let subs = match state.db.all_push_subscriptions() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%e, "failed to load subscriptions");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    let push_body = serde_json::json!({
        "title": payload.title,
        "body": payload.body,
    });
    let push_str = push_body.to_string();

    let mut stale = Vec::new();
    for sub in &subs {
        match send_push(&sub.endpoint, &sub.p256dh, &sub.auth, &push_str, &state.vapid, &state.web_push).await {
            Ok(()) => {}
            Err(web_push::WebPushError::EndpointNotValid) => {
                tracing::warn!(endpoint = %sub.endpoint, "stale subscription, removing");
                stale.push(sub.endpoint.clone());
            }
            Err(e) => {
                tracing::error!(%e, endpoint = %sub.endpoint, "push failed");
            }
        }
    }
    for ep in stale {
        let _ = state.db.delete_push_subscription_by_endpoint(&ep);
    }

    (StatusCode::OK, "OK").into_response()
}

// ── /pull ─────────────────────────────────────────────────────────────────────

const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";

async fn pull_forecast(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let is_admin = params.get("user").map(|u| u.trim().to_lowercase() == "victor").unwrap_or(false);
    if !is_admin {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "forbidden"}))).into_response();
    }
    let cfg = &state.config;
    let url = format!(
        "{}?latitude={}&longitude={}&hourly=windspeed_10m,winddirection_10m,windgusts_10m,temperature_2m,weathercode&wind_speed_unit=kn&forecast_days=2&timezone=America/Chicago",
        OPEN_METEO_URL, cfg.location.lat, cfg.location.lon
    );

    let resp = match state.http.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(%e, "pull: HTTP request failed");
            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

    if !resp.status().is_success() {
        let msg = format!("Open-Meteo returned {}", resp.status());
        return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": msg}))).into_response();
    }

    let raw_json = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

    // Extract valid_from / valid_to from the hourly time array.
    let (valid_from, valid_to) = {
        let v: serde_json::Value = serde_json::from_str(&raw_json).unwrap_or(serde_json::Value::Null);
        let times = v["hourly"]["time"].as_array().cloned().unwrap_or_default();
        let from = times.first().and_then(|t| t.as_str()).unwrap_or("").to_string();
        let to   = times.last().and_then(|t| t.as_str()).unwrap_or("").to_string();
        (from, to)
    };

    let fetched_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    match state.db.insert_forecast(&fetched_at, "open-meteo-hrrr", &valid_from, &valid_to, &raw_json, true) {
        Ok(id) => {
            tracing::info!(forecast_id = id, "manual pull succeeded");
            Json(serde_json::json!({"ok": true, "fetched_at": fetched_at, "forecast_id": id})).into_response()
        }
        Err(e) => {
            tracing::error!(%e, "pull: DB insert failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

// ── /forecast ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ForecastHour {
    time: String,
    wind_kn: f64,
    gusts_kn: f64,
    dir_deg: f64,
    temp_c: Option<f64>,
    wmo: u32,
    rideable: bool,
}

#[derive(Serialize)]
struct ForecastWindow {
    start: String,
    end: String,
    avg_kn: f64,
    dir_deg: f64,
    disciplines: Vec<String>,
}

#[derive(Serialize)]
struct ForecastResponse {
    fetched_at: String,
    source: String,
    valid_from: String,
    valid_to: String,
    hours: Vec<ForecastHour>,
    windows: Vec<ForecastWindow>,
}

fn parse_hours_from_raw(raw_json: &str) -> Vec<ForecastHour> {
    let v: serde_json::Value = match serde_json::from_str(raw_json) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let h = &v["hourly"];
    let times = match h["time"].as_array() {
        Some(a) => a.clone(),
        None => return vec![],
    };
    let winds = h["windspeed_10m"].as_array().cloned().unwrap_or_default();
    let gusts = h["windgusts_10m"].as_array().cloned().unwrap_or_default();
    let dirs = h["winddirection_10m"].as_array().cloned().unwrap_or_default();
    let temps = h["temperature_2m"].as_array().cloned().unwrap_or_default();
    let wmos = h["weathercode"].as_array().cloned().unwrap_or_default();

    times
        .iter()
        .enumerate()
        .map(|(i, t)| ForecastHour {
            time: t.as_str().unwrap_or("").to_string(),
            wind_kn: winds.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            gusts_kn: gusts.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            dir_deg: dirs.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            temp_c: temps.get(i).and_then(|v| v.as_f64()),
            wmo: wmos.get(i).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            rideable: false,
        })
        .collect()
}

async fn forecast(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let row = match state.db.last_forecast() {
        Ok(Some(r)) => r,
        _ => {
            return Json(serde_json::json!({
                "error": "no forecast data yet"
            }))
            .into_response()
        }
    };

    let mut hours = parse_hours_from_raw(&row.raw_json);

    // Load windows from last analysis and mark rideable hours.
    let windows: Vec<ForecastWindow> = state
        .db
        .last_analysis()
        .ok()
        .flatten()
        .and_then(|(_, _, json)| {
            serde_json::from_str::<Vec<serde_json::Value>>(&json).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|w| {
            Some(ForecastWindow {
                start: w["start"].as_str()?.to_string(),
                end: w["end"].as_str()?.to_string(),
                avg_kn: w["avg_kn"].as_f64().unwrap_or(0.0),
                dir_deg: w["dir_deg"].as_f64().unwrap_or(0.0),
                disciplines: w["disciplines"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();

    // Mark rideable hours.
    for hour in &mut hours {
        hour.rideable = windows
            .iter()
            .any(|w| hour.time >= w.start && hour.time <= w.end);
    }

    Json(ForecastResponse {
        fetched_at: row.fetched_at,
        source: row.source,
        valid_from: row.valid_from,
        valid_to: row.valid_to,
        hours,
        windows,
    })
    .into_response()
}

// ── /status ───────────────────────────────────────────────────────────────────

async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let last_forecast = state.db.last_forecast().ok().flatten();
    let last_analysis = state.db.last_analysis().ok().flatten();
    let last_notif = state.db.last_notification_sent().ok().flatten();
    let errors = state.db.errors_count_last_24h().unwrap_or(0);
    let subscribers = state.db.subscribers_count().unwrap_or(0);

    let next_window = last_analysis.as_ref().and_then(|(_, _, json)| {
        let arr: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
        arr.into_iter().next()
    });

    Json(StatusResponse {
        service: "kiteagent",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: 0,
        last_forecast_fetch: last_forecast.map(|f| ForecastFetchInfo {
            at: f.fetched_at,
            source: f.source,
            ok: f.fetch_ok,
        }),
        last_analysis: last_analysis.map(|(at, count, _)| AnalysisInfo {
            at,
            windows_found: count,
            next_window,
        }),
        last_notification_sent: last_notif.map(|(at, ws)| NotificationInfo {
            at,
            window_start: ws,
        }),
        errors_last_24h: errors,
        subscribers,
    })
}
