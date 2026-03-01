use anyhow::Result;
use axum::middleware;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

use hrrr_server::{db, rate_limit, routes, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "hrrr.db".to_string());

    let bind = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8081".to_string());

    let db = db::Db::open(&db_path)?;

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("hrrr-server/0.1 (pigeonstorm.com)")
        .build()?;

    let state = Arc::new(AppState { db, http });

    let app = routes::router(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ));

    let addr: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(bind = %addr, db = %db_path, "hrrr-server listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
