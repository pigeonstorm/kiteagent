//! Fetches live wind from live-server via gRPC and sends notifications when conditions meet user criteria.

use anyhow::Result;
use kiteagent_shared::Config;
use live_server::grpc::proto::{live_weather_client::LiveWeatherClient, GetLatestRequest};
use tonic::transport::Channel;
use tracing::{debug, info};

use crate::notify::send_live_wind_alert;

const MPH_TO_KN: f64 = 0.868976;

fn wind_in_bad_direction(dir_deg: f64, bad_directions: &[Vec<f64>]) -> bool {
    for range in bad_directions {
        if range.len() >= 2 {
            let lo = range[0];
            let hi = range[1];
            if lo <= hi {
                if dir_deg >= lo && dir_deg <= hi {
                    return true;
                }
            } else {
                if dir_deg >= lo || dir_deg <= hi {
                    return true;
                }
            }
        }
    }
    false
}

fn wind_meets_criteria(
    wind_speed_mph: f64,
    wind_dir_deg: i32,
    wind_hi_mph: f64,
    cfg: &Config,
) -> bool {
    let wind_kn = wind_speed_mph * MPH_TO_KN;
    let gust_kn = wind_hi_mph * MPH_TO_KN;
    let gust_ratio = if wind_kn > 0.0 {
        gust_kn / wind_kn
    } else {
        1.0
    };

    if wind_kn < cfg.thresholds.min_wind_kn {
        return false;
    }
    if wind_kn > cfg.thresholds.max_wind_kn {
        return false;
    }
    if gust_ratio > cfg.thresholds.max_gust_ratio {
        return false;
    }
    if wind_in_bad_direction(wind_dir_deg as f64, &cfg.thresholds.bad_directions_deg) {
        return false;
    }
    true
}

pub async fn check_live_wind(
    cfg: &Config,
    db: &kiteagent_shared::Db,
    client: &reqwest::Client,
) -> Result<()> {
    let live_cfg = match &cfg.live {
        Some(l) => l,
        None => {
            debug!("live wind check skipped (no [live] config)");
            return Ok(());
        }
    };

    let channel = Channel::from_shared(live_cfg.grpc_url.clone())?
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("live-server gRPC connect failed: {}", e))?;

    let mut grpc_client = LiveWeatherClient::new(channel);
    let resp = grpc_client
        .get_latest(GetLatestRequest {})
        .await
        .map_err(|e| anyhow::anyhow!("GetLatest failed: {}", e))?;

    let r = resp.into_inner();
    let wind_speed_mph = r.wind_speed_mph;
    let wind_dir_deg = r.wind_direction_deg;
    let wind_dir = r.wind_direction;
    let wind_hi_mph = r.wind_hi_mph;

    let wind_kn = wind_speed_mph * MPH_TO_KN;

    if !wind_meets_criteria(wind_speed_mph, wind_dir_deg, wind_hi_mph, cfg) {
        debug!(
            wind_kn = wind_kn,
            dir = %wind_dir,
            "live wind does not meet criteria"
        );
        return Ok(());
    }

    info!(
        wind_kn = wind_kn,
        dir = %wind_dir,
        "live wind meets criteria, sending notification"
    );

    send_live_wind_alert(
        wind_kn,
        wind_dir_deg as f64,
        &wind_dir,
        r.wind_hi_mph * MPH_TO_KN,
        cfg,
        db,
        client,
    )
    .await?;
    Ok(())
}
