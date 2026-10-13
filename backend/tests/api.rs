//! Smoke test: exercises the real router and an in-memory SQLite database.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use woodmouse::{routes, state::AppState};

async fn app() -> axum::Router {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    db.push_schema().await.expect("create schema");

    routes::api()
        .with_state(AppState::new(db))
        .split_for_parts()
        .0
}

async fn body_string(body: Body) -> String {
    let bytes = body.collect().await.expect("read body").to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

#[tokio::test]
async fn health_and_todos_roundtrip() {
    let app = app().await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"write tests"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = body_string(res.into_body()).await;
    assert!(created.contains("write tests"), "created: {created}");

    let res = app
        .oneshot(
            Request::builder()
                .uri("/todos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let listed = body_string(res.into_body()).await;
    assert!(listed.contains("write tests"), "listed: {listed}");
}
