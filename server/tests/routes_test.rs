//! Integration tests for server routes.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use kiteagent_shared::{Config, Db};
use tower::ServiceExt;
use web_push::WebPushClient;

use kiteagent_server::routes::{router, AppState};
use kiteagent_server::vapid;

fn test_db() -> Db {
    Db::open_in_memory().unwrap()
}

fn test_config() -> Config {
    Config::parse(
        r#"
[location]
name = "t"
lat = 0.0
lon = 0.0
[user]
name = "u"
weight_kg = 70
[gear]
available = []
[notification]
method = "webpush"
server_url = "http://localhost:8080"
push_secret = "secret"
[server]
bind = "127.0.0.1:8080"
vapid_subject = "mailto:victor@pigeonstorm.com"
[schedule]
fetch_interval_min = 60
morning_digest_hour = 7
opportunity_lookahead_hours = 4
notification_cooldown_hours = 4
max_notifications_per_day = 3
[thresholds]
min_wind_kn = 8.0
max_wind_kn = 40.0
max_gust_ratio = 1.6
min_session_hours = 2
[storage]
db_path = ":memory:"
log_dir = "logs"
log_days = 30
"#,
    )
    .unwrap()
}

fn test_vapid() -> vapid::VapidKeys {
    vapid::VapidKeys {
        public_key_pem: String::new(),
        private_key_pem: include_str!("../test_keys/private.pem").to_string(),
        subject: "mailto:victor@pigeonstorm.com".to_string(),
    }
}

fn test_state() -> Arc<AppState> {
    Arc::new(AppState {
        db: test_db(),
        vapid: test_vapid(),
        push_secret: "secret".to_string(),
        config: test_config(),
        http: reqwest::Client::new(),
        web_push: WebPushClient::new().expect("WebPushClient"),
    })
}

#[tokio::test]
async fn doc_returns_200_with_architecture_section() {
    let state = test_state();
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/doc").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("Architecture") && text.contains("mermaid"),
        "doc page should include architecture and mermaid diagram"
    );
}

#[tokio::test]
async fn kite_gear_js_returns_200() {
    let state = test_state();
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/kite-gear.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("javascript"), "content-type should be javascript");
}

#[tokio::test]
async fn kite_gear_wasm_returns_200() {
    let state = test_state();
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/kite-gear.wasm").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("wasm"), "content-type should be wasm");
}

#[tokio::test]
async fn kite_gear_bg_wasm_alias_returns_200() {
    let state = test_state();
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/kite_gear_bg.wasm").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_returns_200() {
    let state = test_state();
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn push_without_bearer_returns_401() {
    let state = test_state();
    let app = router(state);
    let body = serde_json::json!({"title": "Test", "body": "Body"});
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/push")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
