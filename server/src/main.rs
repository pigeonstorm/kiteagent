use anyhow::Result;
use kiteagent_shared::{Config, Db};
use reqwest;
use std::sync::Arc;
use tracing::info;

use kiteagent_server::{routes, vapid};
use kiteagent_server::routes::AppState;

const VAPID_KEYS_PATH: &str = "vapid_keys.json";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let cfg = Config::load_from_path(&config_path)?;

    let db = Db::open(&cfg.storage.db_path)?;
    let vapid = vapid::load_or_create_vapid_keys(VAPID_KEYS_PATH, &cfg.server.vapid_subject)?;

    let state = Arc::new(AppState {
        db,
        vapid,
        push_secret: cfg.notification.push_secret.clone(),
        config: cfg.clone(),
        http: reqwest::Client::new(),
    });

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind).await?;
    info!(bind = %cfg.server.bind, "kiteagent-server listening");

    axum::serve(listener, routes::router(state)).await?;
    Ok(())
}
