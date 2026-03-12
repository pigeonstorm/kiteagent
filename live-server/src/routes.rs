use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::{scrape_and_store, AppState};

// ── error helper ─────────────────────────────────────────────────────────────

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ── router ───────────────────────────────────────────────────────────────────

fn is_admin(params: &HashMap<String, String>) -> bool {
    params
        .get("user")
        .map(|u| u.trim().to_lowercase() == "victor")
        .unwrap_or(false)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/live", get(latest))
        .route("/history", get(history))
        .route("/stats", get(stats))
        .route("/pull", post(pull))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── handlers ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DashboardQuery {
    #[serde(flatten)]
    params: HashMap<String, String>,
}

async fn dashboard(Query(q): Query<DashboardQuery>) -> Html<String> {
    let admin = is_admin(&q.params);
    let html = include_str!("../static/index.html")
        .replace("__IS_ADMIN__", if admin { "true" } else { "false" });
    Html(html)
}

async fn latest(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    match state.db.get_latest()? {
        Some(r) => {
            let mut response = Json(r).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            Ok(response)
        }
        None => Ok((StatusCode::NO_CONTENT, "no readings yet").into_response()),
    }
}

#[derive(Deserialize)]
struct HistoryParams {
    limit: Option<i64>,
}

async fn history(
    State(state): State<Arc<AppState>>,
    Query(p): Query<HistoryParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = p.limit.unwrap_or(100).clamp(1, 1000);
    let readings = state.db.get_history(limit)?;
    Ok(Json(serde_json::json!({ "readings": readings, "count": readings.len() })))
}

async fn stats(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let total = state.db.count()?;
    let latest = state.db.get_latest()?;
    Ok(Json(serde_json::json!({
        "total_readings": total,
        "latest_scraped_at": latest.as_ref().map(|r| r.scraped_at),
        "latest_station_time": latest.as_ref().map(|r| &r.station_time),
    })))
}

// ── POST /pull (admin only) ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct PullQuery {
    #[serde(flatten)]
    params: HashMap<String, String>,
}

async fn pull(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PullQuery>,
) -> ApiResult<Response> {
    if !is_admin(&q.params) {
        return Ok((StatusCode::FORBIDDEN, "admin required (?user=victor)").into_response());
    }
    info!("admin pull: forcing scrape");
    match scrape_and_store(&state).await {
        Ok(r) => Ok(Json(serde_json::json!({
            "ok": true,
            "wind_speed_kn": r.wind_speed_kn,
            "wind_direction": r.wind_direction,
            "scraped_at": r.scraped_at,
        })).into_response()),
        Err(e) => Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response()),
    }
}
