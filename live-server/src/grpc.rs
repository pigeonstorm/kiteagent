use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};
use tracing::info;

use crate::{AppState, WeatherReading};

// Include the tonic-generated code for the `live` package.
pub mod proto {
    tonic::include_proto!("live");
}

use proto::{
    live_weather_server::{LiveWeather, LiveWeatherServer},
    GetHistoryRequest, GetHistoryResponse, GetLatestRequest,
    WeatherReading as ProtoReading,
};

// ── conversion ────────────────────────────────────────────────────────────────

impl From<WeatherReading> for ProtoReading {
    fn from(r: WeatherReading) -> Self {
        ProtoReading {
            id: r.id.unwrap_or(0),
            scraped_at: r.scraped_at.to_rfc3339(),
            station_time: r.station_time,
            wind_speed_kn: r.wind_speed_kn,
            wind_direction: r.wind_direction,
            wind_direction_deg: r.wind_direction_deg,
            wind_avg_kn: r.wind_avg_kn,
            wind_hi_kn: r.wind_hi_kn,
            wind_hi_dir_deg: r.wind_hi_dir_deg,
            wind_rms_kn: r.wind_rms_kn,
            wind_vector_avg_kn: r.wind_vector_avg_kn,
            wind_vector_dir_deg: r.wind_vector_dir_deg,
            temperature_f: r.temperature_f,
            humidity_pct: r.humidity_pct,
            barometer_inhg: r.barometer_inhg,
            barometer_trend: r.barometer_trend.unwrap_or(0.0),
            rain_in: r.rain_in,
            rain_rate_in_hr: r.rain_rate_in_hr,
            wind_chill_f: r.wind_chill_f,
            heat_index_f: r.heat_index_f,
            dewpoint_f: r.dewpoint_f,
        }
    }
}

// ── service implementation ────────────────────────────────────────────────────

pub struct LiveWeatherService {
    state: Arc<AppState>,
}

#[tonic::async_trait]
impl LiveWeather for LiveWeatherService {
    async fn get_latest(
        &self,
        _req: Request<GetLatestRequest>,
    ) -> Result<Response<ProtoReading>, Status> {
        let reading = self
            .state
            .db
            .get_latest()
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("no readings yet"))?;
        Ok(Response::new(reading.into()))
    }

    async fn get_history(
        &self,
        req: Request<GetHistoryRequest>,
    ) -> Result<Response<GetHistoryResponse>, Status> {
        let limit = (req.into_inner().limit as i64).clamp(1, 1000);
        let readings = self
            .state
            .db
            .get_history(limit)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetHistoryResponse {
            readings: readings.into_iter().map(Into::into).collect(),
        }))
    }
}

// ── server launcher ───────────────────────────────────────────────────────────

pub async fn serve(state: Arc<AppState>, addr: &str) -> anyhow::Result<()> {
    let addr = addr.parse()?;
    info!("gRPC listening on {addr}");
    Server::builder()
        .add_service(LiveWeatherServer::new(LiveWeatherService { state }))
        .serve(addr)
        .await?;
    Ok(())
}
