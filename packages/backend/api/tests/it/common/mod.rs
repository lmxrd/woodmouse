//! Shared helpers for the integration tests: the real router on an
//! in-memory SQLite database, plus small request/assertion utilities.
//! Each test binary uses a different subset, so unused helpers are expected.
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use woodmouse::state::AppState;

/// A date-time far enough ahead that nothing seeded with it expires during a test.
pub fn far_future() -> woodmouse::time::DateTime {
    woodmouse::time::DateTime("2100-01-01T00:00:00".parse().expect("valid date-time"))
}

pub async fn app() -> axum::Router {
    app_with_db().await.0
}

/// A fresh database in a temporary file, from the migrations (so the tests also prove they build a
/// working database), search index included.
async fn database() -> sea_orm::DatabaseConnection {
    woodmouse::db::temporary().await
}

fn router(state: AppState) -> axum::Router {
    woodmouse::routes::api(state.clone())
        .with_state(state)
        .split_for_parts()
        .0
}

/// The router with a small upload limit, for testing size checks.
pub async fn app_with_upload_limit(max_bytes: u64) -> (axum::Router, sea_orm::DatabaseConnection) {
    let db = database().await;
    let root = std::env::temp_dir().join(format!("woodmouse-test-{}", uuid::Uuid::new_v4()));
    let files = woodmouse::storage::FileStores::new(
        std::sync::Arc::new(woodmouse::storage::LocalDisk::new(root)),
        max_bytes,
    );
    (router(AppState::with_files(db.clone(), files)), db)
}

/// The router and a handle on its database (the clone shares the pool), to seed users and sessions directly.
pub async fn app_with_db() -> (axum::Router, sea_orm::DatabaseConnection) {
    let db = database().await;
    (router(AppState::new(db.clone())), db)
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

pub fn put_json(uri: &str, json: &str, cookie: Option<&str>) -> Request<Body> {
    with_cookie(
        Request::builder()
            .method("PUT")
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

/// A multipart upload with one `file` part and optionally a `task_id` text part.
pub fn upload(
    uri: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
    task_id: Option<&str>,
    cookie: Option<&str>,
) -> Request<Body> {
    let boundary = "woodmouse-test-boundary";
    let mut body: Vec<u8> = Vec::new();
    if let Some(task_id) = task_id {
        body.extend(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"task_id\"\r\n\r\n{task_id}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    with_cookie(
        Request::builder().method("POST").uri(uri).header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        ),
        cookie,
    )
    .body(Body::from(body))
    .unwrap()
}
