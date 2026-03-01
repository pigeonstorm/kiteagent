pub mod db;
pub mod hrrr;
pub mod rate_limit;
pub mod routes;

pub struct AppState {
    pub db: db::Db,
    pub http: reqwest::Client,
}
