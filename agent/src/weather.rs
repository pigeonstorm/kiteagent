use anyhow::{Context, Result};
use kiteagent_shared::{Config, Db};
use serde::Deserialize;
use tracing::{error, info, warn};

#[cfg(debug_assertions)]
const OPEN_METEO_URL: &str = "http://localhost:8081/forecast";
#[cfg(not(debug_assertions))]
const OPEN_METEO_URL: &str = "https://hrrr.pigeonstorm.com/forecast";

#[derive(Debug, Clone, Deserialize)]
pub struct OpenMeteoResponse {
    pub hourly: OpenMeteoHourly,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenMeteoHourly {
    pub time: Vec<String>,
    pub windspeed_10m: Vec<Option<f64>>,
    pub winddirection_10m: Vec<Option<f64>>,
    pub windgusts_10m: Vec<Option<f64>>,
    pub temperature_2m: Vec<Option<f64>>,
    pub weathercode: Vec<Option<u32>>,
}

#[derive(Debug, Clone)]
pub struct HourlySlot {
    pub time: String,
    pub wind_speed_kn: f64,
    pub wind_direction_deg: f64,
    pub wind_gusts_kn: f64,
    pub temperature_c: Option<f64>,
    pub weather_code: u32,
}

#[derive(Debug, Clone)]
pub struct Forecast {
    pub source: String,
    pub slots: Vec<HourlySlot>,
}

impl Forecast {
    pub fn valid_from(&self) -> Option<&str> {
        self.slots.first().map(|s| s.time.as_str())
    }

    pub fn valid_to(&self) -> Option<&str> {
        self.slots.last().map(|s| s.time.as_str())
    }
}

pub fn parse_open_meteo(resp: OpenMeteoResponse) -> Forecast {
    let n = resp.hourly.time.len();
    let mut slots = Vec::with_capacity(n);
    for i in 0..n {
        let wind_speed_kn = resp.hourly.windspeed_10m.get(i).and_then(|v| *v).unwrap_or(0.0);
        let wind_dir = resp.hourly.winddirection_10m.get(i).and_then(|v| *v).unwrap_or(0.0);
        let gusts = resp.hourly.windgusts_10m.get(i).and_then(|v| *v).unwrap_or(wind_speed_kn);
        let temp = resp.hourly.temperature_2m.get(i).and_then(|v| *v);
        let wmo = resp.hourly.weathercode.get(i).and_then(|v| *v).unwrap_or(0);
        slots.push(HourlySlot {
            time: resp.hourly.time.get(i).cloned().unwrap_or_default(),
            wind_speed_kn,
            wind_direction_deg: wind_dir,
            wind_gusts_kn: gusts,
            temperature_c: temp,
            weather_code: wmo,
        });
    }
    Forecast {
        source: "open-meteo-hrrr".to_string(),
        slots,
    }
}

const MAX_ATTEMPTS: u32 = 3;
const RETRY_DELAYS_SECS: [u64; 2] = [5, 15];

pub async fn fetch_forecast(
    cfg: &Config,
    db: &Db,
    client: &reqwest::Client,
) -> Result<(Forecast, i64)> {
    let url = format!(
        "{}?latitude={}&longitude={}&hourly=windspeed_10m,winddirection_10m,windgusts_10m,temperature_2m,weathercode&wind_speed_unit=kn&forecast_days=2&timezone=America/Chicago",
        OPEN_METEO_URL,
        cfg.location.lat,
        cfg.location.lon
    );

    let start = std::time::Instant::now();
    let mut last_err: Option<String> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        if attempt > 1 {
            let delay = RETRY_DELAYS_SECS[(attempt - 2) as usize];
            warn!(attempt, delay_secs = delay, "retrying Open-Meteo fetch");
            tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
        }

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("Open-Meteo HTTP request failed: {}", e);
                warn!(attempt, error = %msg, "transient request error");
                last_err = Some(msg);
                continue;
            }
        };

        let status = resp.status();

        if status.is_success() {
            let raw_json = resp.text().await.context("read Open-Meteo response body")?;
            let json: OpenMeteoResponse =
                serde_json::from_str(&raw_json).context("parse Open-Meteo JSON")?;
            let forecast = parse_open_meteo(json);
            let valid_from = forecast.valid_from().unwrap_or("");
            let valid_to = forecast.valid_to().unwrap_or("");
            let fetched_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            let forecast_id = db.insert_forecast(
                &fetched_at,
                &forecast.source,
                valid_from,
                valid_to,
                &raw_json,
                true,
            )?;

            info!(
                source = %forecast.source,
                fetched_at = %fetched_at,
                hours_count = forecast.slots.len(),
                duration_ms = start.elapsed().as_millis(),
                attempt,
                "forecast fetched"
            );

            return Ok((forecast, forecast_id));
        }

        let body = resp.text().await.unwrap_or_default();
        let err_msg = format!("Open-Meteo returned {}: {}", status, body);

        if status.is_server_error() {
            warn!(attempt, error = %err_msg, "transient server error, will retry");
            last_err = Some(err_msg);
            continue;
        }

        // Non-retriable error (4xx etc.) — log and bail immediately.
        error!(source = "open-meteo", error = %err_msg, "fetch failed (non-retriable)");
        db.insert_error("weather_fetch", &err_msg, None)?;
        anyhow::bail!("{}", err_msg);
    }

    let err_msg = last_err.unwrap_or_else(|| "unknown error".to_string());
    error!(source = "open-meteo", error = %err_msg, "fetch failed after all retries");
    db.insert_error("weather_fetch", &err_msg, None)?;
    anyhow::bail!("{}", err_msg);
}
