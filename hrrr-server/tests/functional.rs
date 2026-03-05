use axum::middleware;
use hrrr_server::{db::Db, hrrr, rate_limit, routes, AppState};
use std::sync::Arc;

fn test_state() -> Arc<AppState> {
    let db = Db::open_in_memory().unwrap();
    let http = reqwest::Client::new();
    Arc::new(AppState { db, http })
}

fn test_app(state: Arc<AppState>) -> axum::Router {
    routes::router(state.clone()).layer(middleware::from_fn_with_state(
        state,
        rate_limit::rate_limit_middleware,
    ))
}

// ═══════════════════════════════════════════════════════════════════════
// HRRR pure-function tests (zero mocking)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn select_run_48h_picks_extended_cycle() {
    let run = hrrr::select_run(2);
    assert_eq!(run.max_fh, 48);
    assert!(
        [0, 6, 12, 18].contains(&run.cycle),
        "expected extended cycle, got {}",
        run.cycle
    );
}

#[test]
fn select_run_18h_picks_hourly_cycle() {
    let run = hrrr::select_run(1);
    assert_eq!(run.max_fh, 18);
    assert!(run.cycle < 24);
}

#[test]
fn to_openmeteo_json_has_correct_shape() {
    let slots = vec![
        hrrr::HourlySlot {
            time: "2026-02-27T12:00".into(),
            wind_speed_kn: 15.0,
            wind_dir_deg: 225.0,
            wind_gusts_kn: 22.0,
            temperature_c: Some(18.5),
            weather_code: 0,
        },
        hrrr::HourlySlot {
            time: "2026-02-27T13:00".into(),
            wind_speed_kn: 17.3,
            wind_dir_deg: 230.0,
            wind_gusts_kn: 25.1,
            temperature_c: Some(19.0),
            weather_code: 61,
        },
    ];

    let json = hrrr::to_openmeteo_json(&slots, "kn");
    let hourly = &json["hourly"];

    let times = hourly["time"].as_array().unwrap();
    assert_eq!(times.len(), 2);
    assert_eq!(times[0], "2026-02-27T12:00");

    let winds = hourly["windspeed_10m"].as_array().unwrap();
    assert_eq!(winds.len(), 2);
    assert!((winds[0].as_f64().unwrap() - 15.0).abs() < 0.1);

    let dirs = hourly["winddirection_10m"].as_array().unwrap();
    assert_eq!(dirs[1].as_f64().unwrap(), 230.0);

    let gusts = hourly["windgusts_10m"].as_array().unwrap();
    assert!((gusts[1].as_f64().unwrap() - 25.1).abs() < 0.1);

    let temps = hourly["temperature_2m"].as_array().unwrap();
    assert!((temps[0].as_f64().unwrap() - 18.5).abs() < 0.1);

    let codes = hourly["weathercode"].as_array().unwrap();
    assert_eq!(codes[0].as_u64().unwrap(), 0);
    assert_eq!(codes[1].as_u64().unwrap(), 61);
}

#[test]
fn to_openmeteo_json_converts_ms_unit() {
    let slots = vec![hrrr::HourlySlot {
        time: "2026-02-27T12:00".into(),
        wind_speed_kn: 19.44,
        wind_dir_deg: 180.0,
        wind_gusts_kn: 29.16,
        temperature_c: Some(20.0),
        weather_code: 0,
    }];

    let json = hrrr::to_openmeteo_json(&slots, "ms");
    let wind_ms = json["hourly"]["windspeed_10m"][0].as_f64().unwrap();
    assert!(wind_ms < 19.44, "m/s should be smaller than knots: {wind_ms}");
    assert!(wind_ms > 9.0, "m/s value should be reasonable: {wind_ms}");
}

// ═══════════════════════════════════════════════════════════════════════
// Database functional tests (real SQLite in-memory, zero mocking)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn db_cache_upsert_and_retrieve() {
    let db = Db::open_in_memory().unwrap();

    db.upsert_forecast_cache(
        30.46, -97.97, "20260227", 12,
        "2026-02-27T12:50:00Z", "2026-02-27T13:00", "2026-02-28T12:00",
        r#"{"hourly":{"time":["2026-02-27T13:00"]}}"#,
    ).unwrap();

    let cached = db.get_cached_forecast(30.46, -97.97, "20260227", 12).unwrap();
    assert!(cached.is_some());
    let cached = cached.unwrap();
    assert!(cached.raw_json.contains("hourly"));
    assert_eq!(cached.valid_from, "2026-02-27T13:00");
}

#[test]
fn db_cache_upsert_overwrites_same_key() {
    let db = Db::open_in_memory().unwrap();

    db.upsert_forecast_cache(
        30.46, -97.97, "20260227", 12,
        "2026-02-27T12:50:00Z", "a", "b", r#"{"v":1}"#,
    ).unwrap();
    db.upsert_forecast_cache(
        30.46, -97.97, "20260227", 12,
        "2026-02-27T13:50:00Z", "c", "d", r#"{"v":2}"#,
    ).unwrap();

    assert_eq!(db.cache_entry_count().unwrap(), 1);
    let cached = db.get_cached_forecast(30.46, -97.97, "20260227", 12).unwrap().unwrap();
    assert!(cached.raw_json.contains("\"v\":2"));
}

#[test]
fn db_request_log_and_metrics() {
    let db = Db::open_in_memory().unwrap();

    db.log_request("1.2.3.4", "/forecast", Some(30.46), Some(-97.97), true, 200, 5).unwrap();
    db.log_request("1.2.3.4", "/forecast", Some(30.46), Some(-97.97), false, 200, 1200).unwrap();
    db.log_request("5.6.7.8", "/forecast", Some(40.0), Some(-74.0), false, 200, 800).unwrap();

    assert_eq!(db.requests_last_24h().unwrap(), 3);
    assert_eq!(db.requests_last_1h().unwrap(), 3);

    let hit_rate = db.cache_hit_rate_24h().unwrap();
    assert!((hit_rate - 33.33).abs() < 1.0, "expected ~33%, got {hit_rate}");

    let top = db.top_callers_24h(5).unwrap();
    assert_eq!(top[0].0, "1.2.3.4");
    assert_eq!(top[0].1, 2);
}

#[test]
fn db_error_log_and_recent() {
    let db = Db::open_in_memory().unwrap();

    db.log_error("nomads_fetch", "connection timeout").unwrap();
    db.log_error("grib_parse", "unexpected template").unwrap();

    assert_eq!(db.errors_last_24h().unwrap(), 2);

    let recent = db.recent_errors(5).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].kind, "grib_parse");
    assert_eq!(recent[1].kind, "nomads_fetch");
}

#[test]
fn db_rate_limit_count() {
    let db = Db::open_in_memory().unwrap();

    for i in 0..5 {
        db.log_request(&format!("10.0.0.{i}"), "/forecast", None, None, false, 429, 0).unwrap();
    }

    assert_eq!(db.rate_limited_last_24h().unwrap(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// HTTP integration tests (real Axum + real SQLite, only NOMADS is avoided
// by pre-seeding the cache so the handler never reaches the network)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn api_dashboard_returns_html() {
    let state = test_state();
    let app = test_app(state);
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server.get("/").await;
    resp.assert_status_ok();
    let text = resp.text();
    assert!(text.contains("HRRR API"), "dashboard should contain title");
    assert!(text.contains("hrrr.pigeonstorm.com"), "dashboard should contain domain");
}

#[tokio::test]
async fn api_metrics_returns_json_structure() {
    let state = test_state();
    state.db.log_request("1.2.3.4", "/forecast", Some(30.46), Some(-97.97), true, 200, 3).unwrap();
    state.db.log_error("nomads_fetch", "test error").unwrap();

    let app = test_app(state);
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server.get("/metrics.json").await;
    resp.assert_status_ok();

    let body: serde_json::Value = resp.json();
    assert!(body["cache"]["entries"].is_number());
    assert!(body["cache"]["hit_rate_pct"].is_number());
    assert!(body["requests"]["last_24h"].as_i64().unwrap() >= 1);
    assert!(body["errors_last_24h"].as_i64().unwrap() >= 1);
    assert!(body["recent_errors"].as_array().unwrap().len() >= 1);
    assert_eq!(body["recent_errors"][0]["kind"], "nomads_fetch");
}

#[tokio::test]
async fn api_forecast_cache_hit() {
    let state = test_state();

    let run = hrrr::select_run(2);
    let fake_json = serde_json::json!({
        "hourly": {
            "time": ["2026-02-27T13:00"],
            "windspeed_10m": [15.0],
            "winddirection_10m": [225.0],
            "windgusts_10m": [22.0],
            "temperature_2m": [18.5],
            "weathercode": [0]
        }
    });
    state.db.upsert_forecast_cache(
        30.46, -97.97,
        &run.date, run.cycle as i32,
        "2026-02-27T12:50:00Z",
        "2026-02-27T13:00", "2026-02-27T13:00",
        &serde_json::to_string(&fake_json).unwrap(),
    ).unwrap();

    let app = test_app(state.clone());
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server
        .get("/forecast")
        .add_query_param("latitude", "30.46")
        .add_query_param("longitude", "-97.97")
        .add_query_param("wind_speed_unit", "kn")
        .add_query_param("forecast_days", "2")
        .await;

    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["hourly"]["windspeed_10m"][0].as_f64().unwrap(), 15.0);
    assert_eq!(body["hourly"]["winddirection_10m"][0].as_f64().unwrap(), 225.0);

    assert_eq!(state.db.requests_last_1h().unwrap(), 1);
    let hit_rate = state.db.cache_hit_rate_24h().unwrap();
    assert!((hit_rate - 100.0).abs() < 0.1, "should be 100% cache hit, got {hit_rate}");
}

#[tokio::test]
async fn api_forecast_missing_params_returns_error() {
    let state = test_state();
    let app = test_app(state);
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server.get("/forecast").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_rate_limiter_blocks_after_threshold() {
    let state = test_state();

    // Pre-fill request_log with 30 entries from 127.0.0.1 (the test client IP)
    for _ in 0..30 {
        state.db.log_request("127.0.0.1", "/forecast", None, None, false, 200, 1).unwrap();
    }

    let app = test_app(state.clone());
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server
        .get("/forecast")
        .add_query_param("latitude", "30.46")
        .add_query_param("longitude", "-97.97")
        .await;

    resp.assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
    let text = resp.text();
    assert!(text.contains("rate limit exceeded"), "should contain rate limit message");
}

#[tokio::test]
async fn api_rate_limiter_allows_under_threshold() {
    let state = test_state();

    // Only 5 requests -- well under the 30 limit
    for _ in 0..5 {
        state.db.log_request("127.0.0.1", "/forecast", None, None, false, 200, 1).unwrap();
    }

    // Seed cache so the request succeeds without hitting NOMADS
    let run = hrrr::select_run(2);
    state.db.upsert_forecast_cache(
        30.46, -97.97,
        &run.date, run.cycle as i32,
        "2026-02-27T12:50:00Z", "a", "b",
        r#"{"hourly":{"time":["t"],"windspeed_10m":[10],"winddirection_10m":[180],"windgusts_10m":[15],"temperature_2m":[20],"weathercode":[0]}}"#,
    ).unwrap();

    let app = test_app(state);
    let server = axum_test::TestServer::new(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).unwrap();

    let resp = server
        .get("/forecast")
        .add_query_param("latitude", "30.46")
        .add_query_param("longitude", "-97.97")
        .await;

    resp.assert_status_ok();
}
