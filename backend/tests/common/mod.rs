//! Shared helpers for the integration tests: the real router on an
//! in-memory SQLite database, plus small request/assertion utilities.
//! Each test binary uses a different subset, so unused helpers are expected.
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use woodmouse::state::AppState;

pub async fn app() -> axum::Router {
    app_with_db().await.0
}

pub async fn app_with_db() -> (axum::Router, toasty::Db) {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    db.push_schema().await.expect("create schema");

    // The clone shares the pool, so it sees the same in-memory database the
    // router uses — the tests can seed users and sessions directly.
    let state = AppState::new(db.clone());
    let router = woodmouse::routes::api(state.clone())
        .with_state(AppState::new(db.clone()))
        .split_for_parts()
        .0;
    (router, db)
}

pub async fn body_string(body: Body) -> String {
    let bytes = body.collect().await.expect("read body").to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

pub fn cookie_of(res: &axum::http::Response<Body>) -> String {
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .expect("session cookie in response")
        .to_str()
        .expect("ascii cookie header");
    set_cookie
        .split(';')
        .next()
        .expect("cookie value")
        .to_string()
}

pub async fn request(app: &axum::Router, req: Request<Body>) -> axum::http::Response<Body> {
    app.clone().oneshot(req).await.expect("request succeeds")
}

pub fn get(uri: &str, cookie: Option<&str>) -> Request<Body> {
    with_cookie(Request::builder().uri(uri), cookie)
        .body(Body::empty())
        .unwrap()
}

pub fn post_json(uri: &str, json: &str, cookie: Option<&str>) -> Request<Body> {
    with_cookie(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json"),
        cookie,
    )
    .body(Body::from(json.to_string()))
    .unwrap()
}

pub fn patch_json(uri: &str, json: &str, cookie: Option<&str>) -> Request<Body> {
    with_cookie(
        Request::builder()
            .method("PATCH")
            .uri(uri)
            .header("content-type", "application/json"),
        cookie,
    )
    .body(Body::from(json.to_string()))
    .unwrap()
}

pub fn delete(uri: &str, cookie: Option<&str>) -> Request<Body> {
    with_cookie(Request::builder().method("DELETE").uri(uri), cookie)
        .body(Body::empty())
        .unwrap()
}

fn with_cookie(
    builder: axum::http::request::Builder,
    cookie: Option<&str>,
) -> axum::http::request::Builder {
    match cookie {
        Some(cookie) => builder.header("cookie", cookie),
        None => builder,
    }
}

/// Bootstrap the first (super admin) user and return the session cookie.
pub async fn bootstrap_admin(app: &axum::Router) -> String {
    let res = request(
        app,
        post_json(
            "/api/auth/bootstrap",
            r#"{"email":"admin@home.io","name":"Admin","password":"correct horse"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    cookie_of(&res)
}

/// Create a workspace and return its id.
pub async fn create_workspace(app: &axum::Router, cookie: &str, name: &str) -> uuid::Uuid {
    let res = request(
        app,
        post_json(
            "/api/workspaces",
            &format!(r#"{{"name":"{name}"}}"#),
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_string(res.into_body()).await;
    serde_json::from_str::<serde_json::Value>(&body)
        .expect("valid json")
        .get("id")
        .expect("id in response")
        .as_str()
        .expect("id as string")
        .parse()
        .expect("valid uuid")
}
