//! Integration tests for server routes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use kiteagent_shared::Db;
use tower::ServiceExt;

use kiteagent_server::routes::{router, AppState};
use kiteagent_server::vapid;

fn test_db() -> Db {
    Db::open_in_memory().unwrap()
}

fn test_vapid() -> vapid::VapidKeys {
    vapid::VapidKeys {
        public_key_pem: String::new(),
        private_key_pem: include_str!("../test_keys/private.pem").to_string(),
    }
}

#[tokio::test]
async fn status_returns_200() {
    let state = std::sync::Arc::new(AppState {
        db: test_db(),
        vapid: test_vapid(),
        push_secret: "secret".to_string(),
    });
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn push_without_bearer_returns_401() {
    let state = std::sync::Arc::new(AppState {
        db: test_db(),
        vapid: test_vapid(),
        push_secret: "secret".to_string(),
    });
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
