use chrono::Utc;
use live_server::{db, routes, AppState, WeatherReading};
use std::sync::Arc;

fn test_state() -> Arc<AppState> {
    let db = db::Db::open_in_memory().unwrap();
    let http = reqwest::Client::new();
    Arc::new(AppState { db, http })
}

fn seed_one_reading(state: &AppState) {
    let r = WeatherReading {
        id: None,
        scraped_at: Utc::now(),
        station_time: "03/02/2026 11:40:00 PM".into(),
        wind_speed_kn: 8.69,
        wind_direction: "S".into(),
        wind_direction_deg: 180,
        wind_avg_kn: 9.56,
        wind_hi_kn: 30.41,
        wind_hi_dir_deg: 160,
        wind_rms_kn: 9.56,
        wind_vector_avg_kn: 8.69,
        wind_vector_dir_deg: 170,
        temperature_f: 68.2,
        humidity_pct: 85.0,
        barometer_inhg: 29.996,
        barometer_trend: Some(0.019),
        rain_in: 0.0,
        rain_rate_in_hr: 0.0,
        wind_chill_f: 68.2,
        heat_index_f: 68.7,
        dewpoint_f: 63.5,
    };
    state.db.insert_reading(&r).unwrap();
}

#[tokio::test]
async fn api_dashboard_returns_html() {
    let state = test_state();
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/").await;
    resp.assert_status_ok();
    let text = resp.text();
    assert!(text.contains("Lake Travis Live Weather"));
    assert!(text.contains("live.pigeonstorm.com"));
}

#[tokio::test]
async fn api_latest_returns_204_when_empty() {
    let state = test_state();
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/live").await;
    resp.assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn api_latest_returns_json_when_data_exists() {
    let state = test_state();
    seed_one_reading(&state);
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/live").await;
    resp.assert_status_ok();

    let body: serde_json::Value = resp.json();
    assert_eq!(body["wind_speed_kn"].as_f64().unwrap(), 8.69);
    assert_eq!(body["wind_direction"], "S");
    assert_eq!(body["temperature_f"].as_f64().unwrap(), 68.2);
}

#[tokio::test]
async fn api_history_returns_json() {
    let state = test_state();
    seed_one_reading(&state);
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/history").await;
    resp.assert_status_ok();

    let body: serde_json::Value = resp.json();
    assert!(body["readings"].is_array());
    assert_eq!(body["count"].as_u64().unwrap(), 1);
    assert_eq!(body["readings"][0]["wind_speed_kn"].as_f64().unwrap(), 8.69);
}

#[tokio::test]
async fn api_history_respects_limit_param() {
    let state = test_state();
    seed_one_reading(&state);
    seed_one_reading(&state);
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/history").add_query_param("limit", "1").await;
    resp.assert_status_ok();

    let body: serde_json::Value = resp.json();
    assert_eq!(body["count"].as_u64().unwrap(), 1);
    assert_eq!(body["readings"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn api_stats_returns_json() {
    let state = test_state();
    seed_one_reading(&state);
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.get("/stats").await;
    resp.assert_status_ok();

    let body: serde_json::Value = resp.json();
    assert!(body["total_readings"].is_number());
    assert!(body["total_readings"].as_i64().unwrap() >= 1);
    assert!(body["latest_scraped_at"].is_string());
    assert!(body["latest_station_time"].is_string());
}

#[tokio::test]
async fn api_pull_returns_403_without_admin() {
    let state = test_state();
    let app = routes::router(state);
    let server =
        axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .unwrap();

    let resp = server.post("/pull").await;
    resp.assert_status(axum::http::StatusCode::FORBIDDEN);
}
