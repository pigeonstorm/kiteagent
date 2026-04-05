use anyhow::Result;
use axum::middleware;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

use hrrr_server::{db, rate_limit, routes, AppState};

async fn run_pull_cli(db_path: &str) -> Result<()> {
    let db = db::Db::open(db_path)?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("hrrr-server/0.1 (pigeonstorm.com)")
        .build()?;

    let (run, hours, _) = hrrr_server::pull_forecast_cache(
        &http,
        &db,
        hrrr_server::DEFAULT_PULL_LAT,
        hrrr_server::DEFAULT_PULL_LON,
        hrrr_server::DEFAULT_FORECAST_DAYS,
    )
    .await?;

    info!(
        run_date = %run.date,
        run_cycle = run.cycle,
        hours,
        db = %db_path,
        "hrrr-server pull: forecast cached"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s.as_str()) == Some("pull") {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("info".parse()?),
            )
            .init();
        let db_path = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "hrrr.db".to_string());
        return run_pull_cli(&db_path).await;
    }

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

    let db_path = args
        .get(0)
        .cloned()
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
