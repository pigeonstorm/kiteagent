
use anyhow::Result;
use kiteagent_shared::{Config, Db};
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use kiteagent_agent::conditions::{evaluate, RideableWindow};
use kiteagent_agent::live_wind::check_live_wind;
use kiteagent_agent::notify::{send_morning_digest, send_opportunity_alert};
use kiteagent_agent::weather::fetch_forecast;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn init_tracing(log_dir: &str) -> Result<()> {
    std::fs::create_dir_all(log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(log_dir, "kiteagent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .boxed();
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_target(true)
        .boxed();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .with(file_layer)
        .with(stdout_layer)
        .init();
    Ok(())
}

async fn run_fetch_and_evaluate(
    cfg: Arc<Config>,
    db: Arc<Db>,
    client: Arc<reqwest::Client>,
) -> Result<()> {
    let (forecast, forecast_id) = fetch_forecast(&cfg, &db, &client).await?;
    let windows = evaluate(&forecast, &cfg);
    let result_json = serde_json::to_string(&windows)?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.insert_analysis_run(forecast_id, &now, windows.len() as i32, &result_json)?;

    let lookahead_hours = cfg.schedule.opportunity_lookahead_hours;
    let now_dt = chrono::Utc::now();
    for w in &windows {
        if window_starts_within_hours(&w.start, lookahead_hours as i64, &now_dt) {
            send_opportunity_alert(w, &cfg, &db, &client).await?;
        }
    }
    Ok(())
}

fn window_starts_within_hours(start: &str, hours: i64, now: &chrono::DateTime<chrono::Utc>) -> bool {
    let start_dt = match chrono::DateTime::parse_from_rfc3339(start) {
        Ok(d) => d.with_timezone(&chrono::Utc),
        Err(_) => return false,
    };
    let diff = start_dt - *now;
    diff >= chrono::Duration::hours(0) && diff <= chrono::Duration::hours(hours)
}

async fn run_morning_digest(
    cfg: Arc<Config>,
    db: Arc<Db>,
    client: Arc<reqwest::Client>,
) -> Result<()> {
    let windows: Vec<RideableWindow> = match db.last_analysis() {
        Ok(Some((_, _, json))) => serde_json::from_str(&json).unwrap_or_default(),
        _ => Vec::new(),
    };
    send_morning_digest(&windows, &cfg, &db, &client).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let cfg = Config::load_from_path(&config_path)?;
    init_tracing(&cfg.storage.log_dir)?;

    info!(
        version = VERSION,
        config_path = %config_path,
        db_path = %cfg.storage.db_path,
        "kiteagent-agent starting"
    );

    let db = Arc::new(Db::open(&cfg.storage.db_path)?);
    let cfg = Arc::new(cfg);
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
    );

    let scheduler = JobScheduler::new().await?;
    let cfg1 = Arc::clone(&cfg);
    let db1 = Arc::clone(&db);
    let client1 = Arc::clone(&client);
    scheduler
        .add(
            Job::new_async("0 0 */6 * * *", move |_uuid, _lock| {
                let cfg = Arc::clone(&cfg1);
                let db = Arc::clone(&db1);
                let client = Arc::clone(&client1);
                Box::pin(async move {
                    if let Err(e) = run_fetch_and_evaluate(cfg, db, client).await {
                        tracing::error!(%e, "fetch_and_evaluate failed");
                    }
                })
            })?
        )
        .await?;

    let cfg2 = Arc::clone(&cfg);
    let db2 = Arc::clone(&db);
    let client2 = Arc::clone(&client);
    scheduler
        .add(
            Job::new_async("0 30 13 * * *", move |_uuid, _lock| {
                let cfg = Arc::clone(&cfg2);
                let db = Arc::clone(&db2);
                let client = Arc::clone(&client2);
                Box::pin(async move {
                    if let Err(e) = run_morning_digest(cfg, db, client).await {
                        tracing::error!(%e, "morning_digest failed");
                    }
                })
            })?
        )
        .await?;

    let cfg3 = Arc::clone(&cfg);
    let db3 = Arc::clone(&db);
    let client3 = Arc::clone(&client);
    scheduler
        .add(
            Job::new_async("0 */30 * * * *", move |_uuid, _lock| {
                let cfg = Arc::clone(&cfg3);
                let db = Arc::clone(&db3);
                let client = Arc::clone(&client3);
                Box::pin(async move {
                    if let Err(e) = check_live_wind(&cfg, &db, &client).await {
                        tracing::error!(%e, "live_wind check failed");
                    }
                })
            })?
        )
        .await?;

    scheduler.start().await?;
    info!("scheduler started (6-hourly fetch, 7:30am CST digest, 30-min live wind)");

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }
}
