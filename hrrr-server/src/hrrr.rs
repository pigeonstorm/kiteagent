use anyhow::{Context, Result};
use chrono::{Timelike, Utc};
use serde::Serialize;
use std::io::Cursor;
use tracing::{debug, warn};

const NOMADS_FILTER: &str = "https://nomads.ncep.noaa.gov/cgi-bin/filter_hrrr_2d.pl";
const MAX_CONCURRENT: usize = 10;
const NOMADS_RETRY: usize = 2;

#[derive(Debug, Clone, Serialize)]
pub struct HourlySlot {
    pub time: String,
    pub wind_speed_kn: f64,
    pub wind_dir_deg: f64,
    pub wind_gusts_kn: f64,
    pub temperature_c: Option<f64>,
    pub weather_code: u32,
}

#[derive(Debug, Clone)]
pub struct HrrrRun {
    pub date: String,
    pub cycle: u32,
    pub max_fh: u32,
}

/// Determine the best HRRR run to use.
/// Extended runs (00/06/12/18z) go to f48; hourly runs go to f18.
/// NOMADS data is typically available ~50 min after cycle time.
pub fn select_run(forecast_days: u32) -> HrrrRun {
    let now = Utc::now();
    let utc_hour = now.hour();

    let needed_fh: u32 = if forecast_days >= 2 { 48 } else { 18 };

    if needed_fh > 18 {
        let extended_cycles = [18, 12, 6, 0];
        for &cycle in &extended_cycles {
            if utc_hour > cycle || (utc_hour == cycle && now.minute() >= 50) {
                return HrrrRun {
                    date: now.format("%Y%m%d").to_string(),
                    cycle,
                    max_fh: 48,
                };
            }
        }
        let yesterday = now - chrono::Duration::hours(24);
        return HrrrRun {
            date: yesterday.format("%Y%m%d").to_string(),
            cycle: 18,
            max_fh: 48,
        };
    }

    let safe_cycle = if utc_hour == 0 { 0 } else { utc_hour - 1 };
    HrrrRun {
        date: now.format("%Y%m%d").to_string(),
        cycle: safe_cycle,
        max_fh: 18,
    }
}

fn build_nomads_url(run: &HrrrRun, fh: u32, lat: f64, lon: f64) -> String {
    let margin = 0.15;
    format!(
        "{NOMADS_FILTER}\
         ?file=hrrr.t{:02}z.wrfsfcf{:02}.grib2\
         &var_UGRD=on&var_VGRD=on&var_GUST=on&var_TMP=on\
         &var_CRAIN=on&var_CSNOW=on&var_CFRZR=on&var_CICEP=on\
         &lev_10_m_above_ground=on&lev_surface=on&lev_2_m_above_ground=on\
         &subregion=&leftlon={:.2}&rightlon={:.2}&toplat={:.2}&bottomlat={:.2}\
         &dir=%2Fhrrr.{}%2Fconus",
        run.cycle,
        fh,
        lon - margin,
        lon + margin,
        lat + margin,
        lat - margin,
        run.date,
    )
}

fn compute_forecast_time(run: &HrrrRun, fh: u32) -> String {
    let date = chrono::NaiveDate::parse_from_str(&run.date, "%Y%m%d").unwrap();
    let base = date
        .and_hms_opt(run.cycle, 0, 0)
        .unwrap();
    let valid = base + chrono::Duration::hours(fh as i64);
    valid.format("%Y-%m-%dT%H:%M").to_string()
}

const MPS_TO_KN: f64 = 1.94384;

fn extract_values_from_grib2(data: &[u8]) -> Result<GribValues> {
    let cursor = Cursor::new(data);
    let grib2 = grib::from_reader(cursor)
        .context("failed to parse GRIB2 data")?;

    let mut ugrd: Option<f64> = None;
    let mut vgrd: Option<f64> = None;
    let mut gust: Option<f64> = None;
    let mut tmp: Option<f64> = None;
    let mut crain: Option<f64> = None;
    let mut csnow: Option<f64> = None;
    let mut cfrzr: Option<f64> = None;
    let mut cicep: Option<f64> = None;

    for (_idx, submsg) in grib2.iter() {
        let prod = submsg.prod_def();
        let category: Option<u8> = prod.parameter_category();
        let number: Option<u8> = prod.parameter_number();

        let surfaces = prod.fixed_surfaces();
        let level_type = surfaces.as_ref().map(|(first, _)| first.surface_type);
        let level_val = surfaces.as_ref().and_then(|(first, _)| {
            if first.scaled_value != i32::MIN {
                Some(first.scaled_value)
            } else {
                None
            }
        });

        let decoder = match grib::Grib2SubmessageDecoder::from(submsg) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let values: Vec<f64> = match decoder.dispatch() {
            Ok(v) => v.map(|f| f as f64).collect(),
            Err(_) => continue,
        };

        if values.is_empty() {
            continue;
        }

        let center_val = values[values.len() / 2];

        match (category, number, level_type, level_val) {
            // UGRD at 10m above ground (category=2, number=2, level_type=103, level=10)
            (Some(2), Some(2), Some(103), Some(10)) => ugrd = Some(center_val),
            // VGRD at 10m above ground (category=2, number=3, level_type=103, level=10)
            (Some(2), Some(3), Some(103), Some(10)) => vgrd = Some(center_val),
            // GUST at surface (category=2, number=22, level_type=1)
            (Some(2), Some(22), Some(1), _) => gust = Some(center_val),
            // TMP at 2m above ground (category=0, number=0, level_type=103, level=2)
            (Some(0), Some(0), Some(103), Some(2)) => tmp = Some(center_val),
            // CRAIN (category=1, number=192, level_type=1)
            (Some(1), Some(192), Some(1), _) => crain = Some(center_val),
            // CSNOW (category=1, number=195, level_type=1)
            (Some(1), Some(195), Some(1), _) => csnow = Some(center_val),
            // CFRZR (category=1, number=193, level_type=1)
            (Some(1), Some(193), Some(1), _) => cfrzr = Some(center_val),
            // CICEP (category=1, number=194, level_type=1)
            (Some(1), Some(194), Some(1), _) => cicep = Some(center_val),
            _ => {}
        }
    }

    Ok(GribValues {
        ugrd,
        vgrd,
        gust,
        tmp_k: tmp,
        crain,
        csnow,
        cfrzr,
        cicep,
    })
}

#[derive(Debug, Default)]
struct GribValues {
    ugrd: Option<f64>,
    vgrd: Option<f64>,
    gust: Option<f64>,
    tmp_k: Option<f64>,
    crain: Option<f64>,
    csnow: Option<f64>,
    cfrzr: Option<f64>,
    cicep: Option<f64>,
}

impl GribValues {
    fn to_hourly_slot(&self, time: &str) -> HourlySlot {
        let u = self.ugrd.unwrap_or(0.0);
        let v = self.vgrd.unwrap_or(0.0);
        let speed_ms = (u * u + v * v).sqrt();
        let speed_kn = speed_ms * MPS_TO_KN;
        let dir_deg = ((-u).atan2(-v).to_degrees() + 360.0) % 360.0;

        let gust_kn = self.gust.map(|g| g * MPS_TO_KN).unwrap_or(speed_kn);
        let temp_c = self.tmp_k.map(|k| k - 273.15);

        let wmo = synthesize_wmo(self.crain, self.csnow, self.cfrzr, self.cicep);

        HourlySlot {
            time: time.to_string(),
            wind_speed_kn: (speed_kn * 10.0).round() / 10.0,
            wind_dir_deg: dir_deg.round(),
            wind_gusts_kn: (gust_kn * 10.0).round() / 10.0,
            temperature_c: temp_c.map(|t| (t * 10.0).round() / 10.0),
            weather_code: wmo,
        }
    }
}

fn synthesize_wmo(
    crain: Option<f64>,
    csnow: Option<f64>,
    cfrzr: Option<f64>,
    cicep: Option<f64>,
) -> u32 {
    let rain = crain.unwrap_or(0.0) > 0.5;
    let snow = csnow.unwrap_or(0.0) > 0.5;
    let frzr = cfrzr.unwrap_or(0.0) > 0.5;
    let icep = cicep.unwrap_or(0.0) > 0.5;

    if frzr { return 66; }
    if icep { return 67; }
    if snow { return 71; }
    if rain { return 61; }
    0
}

async fn download_one(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>> {
    for attempt in 0..NOMADS_RETRY {
        if attempt > 0 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        match client.get(url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let bytes = resp.bytes().await.context("read NOMADS response")?;
                    return Ok(bytes.to_vec());
                }
                let status = resp.status();
                if status.is_server_error() {
                    warn!(attempt, status = %status, "NOMADS 5xx, retrying");
                    continue;
                }
                anyhow::bail!("NOMADS returned {status}");
            }
            Err(e) => {
                warn!(attempt, error = %e, "NOMADS request error, retrying");
                if attempt + 1 >= NOMADS_RETRY {
                    return Err(e.into());
                }
            }
        }
    }
    anyhow::bail!("NOMADS download failed after {NOMADS_RETRY} attempts");
}

/// Fetch a full HRRR forecast from NOMADS, returning hourly slots.
pub async fn fetch_hrrr(
    client: &reqwest::Client,
    lat: f64,
    lon: f64,
    forecast_days: u32,
) -> Result<(HrrrRun, Vec<HourlySlot>)> {
    let run = select_run(forecast_days);
    debug!(date = %run.date, cycle = run.cycle, max_fh = run.max_fh, "selected HRRR run");

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
    let mut handles = Vec::new();

    for fh in 1..=run.max_fh {
        let url = build_nomads_url(&run, fh, lat, lon);
        let time = compute_forecast_time(&run, fh);
        let client = client.clone();
        let permit = sem.clone();

        handles.push(tokio::spawn(async move {
            let _permit = permit.acquire().await.unwrap();
            let data = download_one(&client, &url).await;
            (fh, time, data)
        }));
    }

    let mut slots = Vec::new();
    let mut errors = Vec::new();

    for handle in handles {
        let (fh, time, result) = handle.await.context("task join")?;
        match result {
            Ok(data) => {
                match extract_values_from_grib2(&data) {
                    Ok(vals) => slots.push((fh, vals.to_hourly_slot(&time))),
                    Err(e) => {
                        warn!(fh, error = %e, "GRIB2 parse failed for forecast hour");
                        errors.push(format!("f{fh:02}: {e}"));
                    }
                }
            }
            Err(e) => {
                warn!(fh, error = %e, "download failed for forecast hour");
                errors.push(format!("f{fh:02}: {e}"));
            }
        }
    }

    if slots.is_empty() {
        anyhow::bail!(
            "all forecast hours failed: {}",
            errors.join("; ")
        );
    }

    slots.sort_by_key(|(fh, _)| *fh);
    let slots: Vec<HourlySlot> = slots.into_iter().map(|(_, s)| s).collect();

    Ok((run, slots))
}

/// Build Open-Meteo-compatible JSON from hourly slots.
pub fn to_openmeteo_json(
    slots: &[HourlySlot],
    wind_speed_unit: &str,
) -> serde_json::Value {
    let kn_factor: f64 = match wind_speed_unit {
        "kn" => 1.0,
        "ms" => 1.0 / MPS_TO_KN,
        "kmh" => 3.6 / MPS_TO_KN,
        "mph" => 1.0 / MPS_TO_KN * 2.23694,
        _ => 1.0,
    };

    let times: Vec<&str> = slots.iter().map(|s| s.time.as_str()).collect();
    let winds: Vec<f64> = slots.iter().map(|s| (s.wind_speed_kn * kn_factor * 10.0).round() / 10.0).collect();
    let dirs: Vec<f64> = slots.iter().map(|s| s.wind_dir_deg).collect();
    let gusts: Vec<f64> = slots.iter().map(|s| (s.wind_gusts_kn * kn_factor * 10.0).round() / 10.0).collect();
    let temps: Vec<Option<f64>> = slots.iter().map(|s| s.temperature_c).collect();
    let codes: Vec<u32> = slots.iter().map(|s| s.weather_code).collect();

    serde_json::json!({
        "hourly": {
            "time": times,
            "windspeed_10m": winds,
            "winddirection_10m": dirs,
            "windgusts_10m": gusts,
            "temperature_2m": temps,
            "weathercode": codes,
        }
    })
}
