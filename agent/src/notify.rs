use anyhow::Result;
use kiteagent_shared::{Config, Db};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::conditions::RideableWindow;
use crate::gear::recommend;

fn message_hash(window_start: &str, disciplines: &[String], wind_bucket: f64) -> String {
    let mut sorted: Vec<_> = disciplines.iter().map(|s| s.as_str()).collect();
    sorted.sort();
    let disc_str = sorted.join(",");
    let input = format!("{}|{}|{:.1}", window_start, disc_str, wind_bucket);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn wind_bucket(wind_kn: f64) -> f64 {
    (wind_kn / 3.0).floor() * 3.0
}

fn live_wind_message_hash(wind_bucket: f64, dir_bucket: i32) -> String {
    let input = format!("live_wind|{:.1}|{}", wind_bucket, dir_bucket);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn send_live_wind_alert(
    wind_kn: f64,
    dir_deg: f64,
    dir_cardinal: &str,
    gusts_kn: f64,
    cfg: &Config,
    db: &Db,
    client: &reqwest::Client,
) -> Result<bool> {
    let cooldown = cfg.schedule.notification_cooldown_hours as i64;
    let wb = wind_bucket(wind_kn);
    let dir_bucket = ((dir_deg / 22.5) + 0.5) as i32 % 16;
    let hash = live_wind_message_hash(wb, dir_bucket);

    if db.notification_recently_sent(&hash, cooldown)? {
        debug!(hash = %hash, "live wind notification skipped (dedup)");
        return Ok(false);
    }

    let count_today = db.notifications_count_today()?;
    if count_today >= cfg.schedule.max_notifications_per_day as i64 {
        debug!(
            count = count_today,
            max = cfg.schedule.max_notifications_per_day,
            "live wind notification skipped (daily cap)"
        );
        return Ok(false);
    }

    let title = "Live wind at Lake Travis";
    let body = format!(
        "Wind: {:.0} kn (gusts {:.0} kn) | Direction: {} ({:.0}°)",
        wind_kn, gusts_kn, dir_cardinal, dir_deg
    );

    send_push(cfg, title, &body, client).await?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.insert_notification_sent(
        &now,
        &now,
        &now,
        wind_kn,
        dir_deg,
        "live",
        &hash,
    )?;

    info!(
        wind_kn = wind_kn,
        dir = %dir_cardinal,
        "live wind notification sent"
    );
    Ok(true)
}

pub async fn send_opportunity_alert(
    window: &RideableWindow,
    cfg: &Config,
    db: &Db,
    client: &reqwest::Client,
) -> Result<bool> {
    let cooldown = cfg.schedule.notification_cooldown_hours as i64;
    let hash = message_hash(
        &window.start,
        &window.disciplines,
        wind_bucket(window.avg_kn),
    );

    if db.notification_recently_sent(&hash, cooldown)? {
        debug!(hash = %hash, "notification skipped (dedup)");
        return Ok(false);
    }

    let count_today = db.notifications_count_today()?;
    if count_today >= cfg.schedule.max_notifications_per_day as i64 {
        debug!(
            count = count_today,
            max = cfg.schedule.max_notifications_per_day,
            "notification skipped (daily cap)"
        );
        return Ok(false);
    }

    let gear = recommend(window, cfg);
    let dir_cardinal = deg_to_cardinal(window.dir_deg);
    let title = format!(
        "Kite window: {} – {}",
        format_time_short(&window.start),
        format_time_short(&window.end)
    );
    let body = format!(
        "Wind: {:.0} kn avg, gusts {:.0} kn | Direction: {} ({:.0}°)\n\n{}",
        window.avg_kn,
        window.avg_kn * 1.4,
        dir_cardinal,
        window.dir_deg,
        gear
    );

    send_push(cfg, &title, &body, client).await?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.insert_notification_sent(
        &now,
        &window.start,
        &window.end,
        window.avg_kn,
        window.dir_deg,
        &window.disciplines.join(","),
        &hash,
    )?;

    info!(
        window_start = %window.start,
        window_end = %window.end,
        disciplines = ?window.disciplines,
        "notification sent"
    );
    Ok(true)
}

pub async fn send_morning_digest(
    windows: &[RideableWindow],
    cfg: &Config,
    db: &Db,
    client: &reqwest::Client,
) -> Result<bool> {
    let count_today = db.notifications_count_today()?;
    if count_today >= cfg.schedule.max_notifications_per_day as i64 {
        debug!("morning digest skipped (daily cap)");
        return Ok(false);
    }

    // Window timestamps are in America/Chicago local time (fetched with timezone=America/Chicago).
    // At 7:30 AM CST (13:30 UTC) the UTC-6 date always matches the local date.
    let cst_now = chrono::Utc::now() - chrono::Duration::hours(6);
    let today = cst_now.format("%Y-%m-%d").to_string();
    let tomorrow = (cst_now + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();

    let windows_today: Vec<&RideableWindow> = windows
        .iter()
        .filter(|w| w.start.starts_with(&today))
        .collect();
    let windows_tomorrow: Vec<&RideableWindow> = windows
        .iter()
        .filter(|w| w.start.starts_with(&tomorrow))
        .collect();

    let format_day = |day_windows: &[&RideableWindow]| -> String {
        if day_windows.is_empty() {
            "  Not windy.".to_string()
        } else {
            day_windows
                .iter()
                .map(|w| {
                    let gear = recommend(*w, cfg);
                    format!(
                        "  {} – {}: {:.0}kn {}\n  {}",
                        format_time_short(&w.start),
                        format_time_short(&w.end),
                        w.avg_kn,
                        deg_to_cardinal(w.dir_deg),
                        gear
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    };

    let title = "Morning kite forecast";
    let body = format!(
        "Today:\n{}\n\nTomorrow:\n{}",
        format_day(&windows_today),
        format_day(&windows_tomorrow),
    );

    send_push(cfg, title, &body, client).await?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let hash = format!("morning_{}", chrono::Utc::now().format("%Y-%m-%d"));
    let first = windows.first();
    db.insert_notification_sent(
        &now,
        first.map(|w| w.start.as_str()).unwrap_or(""),
        first.map(|w| w.end.as_str()).unwrap_or(""),
        first.map(|w| w.avg_kn).unwrap_or(0.0),
        first.map(|w| w.dir_deg).unwrap_or(0.0),
        "",
        &hash,
    )?;

    info!(windows_count = windows.len(), "morning digest sent");
    Ok(true)
}

async fn send_push(cfg: &Config, title: &str, body: &str, client: &reqwest::Client) -> Result<()> {
    let url = format!("{}/push", cfg.notification.server_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "title": title,
        "body": body,
        "priority": "high"
    });
    let resp = client
        .post(&url)
        .header(
            "Authorization",
            format!("Bearer {}", cfg.notification.push_secret),
        )
        .json(&payload)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("push server returned {}: {}", resp.status(), resp.text().await?);
    }
    Ok(())
}

fn format_time_short(s: &str) -> String {
    if s.len() >= 16 {
        format!("{}", &s[11..16])
    } else {
        s.to_string()
    }
}

fn deg_to_cardinal(deg: f64) -> &'static str {
    let d = ((deg / 22.5) + 0.5) as i32 % 16;
    ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"][d as usize]
}
