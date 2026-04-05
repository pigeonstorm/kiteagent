use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use live_server::{db, grpc, routes, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s.as_str()) == Some("pull") {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("live_server=debug".parse()?)
                    .add_directive("info".parse()?),
            )
            .init();
        let db_arg = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "live.db".to_string());
        let db_path = live_server::resolve_db_path(&db_arg);
        info!("database: {}", db_path);
        let r = live_server::pull_once(&db_arg).await?;
        info!(
            wind_kn = r.wind_speed_kn,
            dir = %r.wind_direction,
            "live-server pull: stored reading"
        );
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("live_server=debug".parse()?)
                .add_directive("info".parse()?),
        )
        .init();

    let db_path_arg = args
        .get(0)
        .cloned()
        .unwrap_or_else(|| "live.db".to_string());

    let db_path = live_server::resolve_db_path(&db_path_arg);
    info!("database: {}", db_path);

    let http_bind =
        std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8082".to_string());

    let grpc_bind =
        std::env::var("GRPC_BIND").unwrap_or_else(|_| "0.0.0.0:50051".to_string());

    let scrape_interval_secs: u64 = std::env::var("SCRAPE_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);

    let db = db::Db::open(&db_path)?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("live-server/0.1 (pigeonstorm.com)")
        .build()?;

    let state = Arc::new(AppState { db, http });

    // Background scraper — runs every `scrape_interval_secs` seconds.
    tokio::spawn(live_server::scrape_loop(
        state.clone(),
        scrape_interval_secs,
    ));

    // gRPC server on a separate port.
    let grpc_state = state.clone();
    let grpc_bind_clone = grpc_bind.clone();
    tokio::spawn(async move {
        if let Err(e) = grpc::serve(grpc_state, &grpc_bind_clone).await {
            tracing::error!("gRPC server error: {e:#}");
        }
    });

    // HTTP server (main task).
    let listener = TcpListener::bind(&http_bind).await?;
    info!("HTTP listening on {http_bind}  gRPC on {grpc_bind}");
    axum::serve(
        listener,
        routes::router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
