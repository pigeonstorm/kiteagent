use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::AppState;

const WINDOW_SECS: i64 = 60;
const MAX_REQUESTS: i64 = 30;

pub async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let ip = addr.ip().to_string();
    let since = (chrono::Utc::now() - chrono::Duration::seconds(WINDOW_SECS))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let count = state
        .db
        .count_requests_for_ip_since(&ip, &since)
        .unwrap_or(0);

    if count >= MAX_REQUESTS {
        let retry_after = WINDOW_SECS.to_string();
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", retry_after.as_str())],
            "rate limit exceeded",
        )
            .into_response();
    }

    next.run(request).await
}
