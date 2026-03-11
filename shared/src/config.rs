use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub location: LocationConfig,
    pub user: UserConfig,
    pub gear: GearConfig,
    pub notification: NotificationConfig,
    pub server: ServerConfig,
    pub schedule: ScheduleConfig,
    pub thresholds: ThresholdsConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub live: Option<LiveConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveConfig {
    pub grpc_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocationConfig {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub weight_kg: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GearConfig {
    pub available: Vec<String>,
    #[serde(default)]
    pub kites: KiteSizes,
    #[serde(default)]
    pub windfoil_sails: SailSizes,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KiteSizes {
    #[serde(default)]
    pub sizes: Vec<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SailSizes {
    #[serde(default)]
    pub sizes: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationConfig {
    pub method: String,
    pub server_url: String,
    pub push_secret: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub vapid_subject: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleConfig {
    pub fetch_interval_min: u32,
    pub morning_digest_hour: u32,
    pub opportunity_lookahead_hours: u32,
    pub notification_cooldown_hours: u32,
    pub max_notifications_per_day: u32,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Deserialize)]
pub struct ThresholdsConfig {
    pub min_wind_kn: f64,
    pub max_wind_kn: f64,
    pub max_gust_ratio: f64,
    pub min_session_hours: u32,
    #[serde(default)]
    pub bad_directions_deg: Vec<Vec<f64>>,
    /// Only flag rideable windows during civil daylight (sunrise → sunset).
    #[serde(default = "default_true")]
    pub daylight_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub db_path: String,
    pub log_dir: String,
    pub log_days: u32,
}

impl Config {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read config from {:?}", path.as_ref()))?;
        Self::parse(&contents)
    }

    pub fn parse(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse config TOML")
    }

    pub fn kite_sizes(&self) -> &[f64] {
        &self.gear.kites.sizes
    }

    pub fn sail_sizes(&self) -> &[f64] {
        &self.gear.windfoil_sails.sizes
    }

    pub fn available_disciplines(&self) -> &[String] {
        &self.gear.available
    }
}
