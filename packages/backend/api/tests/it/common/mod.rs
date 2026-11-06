//! Shared helpers for the integration tests: the real router on a fresh SQLite database, driven through
//! axum-test's `TestServer` (in memory, no socket). Each test file uses a different subset, so unused
//! helpers are expected.
//!
//! Most tests act as several people at once, so a request carries its person's session cookie explicitly
//! (`.session(Some(&cookie))`). A server that keeps cookies between requests, as a browser does, is
//! [`browser`].
#![allow(dead_code)]

use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode};
pub use axum_test::multipart::{MultipartForm, Part};
pub use axum_test::{TestRequest, TestResponse, TestServer};

use api::state::AppState;

/// The app under test. Shared (`Arc`) so concurrent requests can be spawned.
pub type App = Arc<TestServer>;

/// A date-time far enough ahead that nothing seeded with it expires during a test.
pub fn far_future() -> api::time::DateTime {
    api::time::DateTime("2100-01-01T00:00:00".parse().expect("valid date-time"))
}

/// A fresh database in a temporary file, from the migrations (so the tests also prove they build a
/// working database), search index included.
async fn database() -> sea_orm::DatabaseConnection {
    api::db::temporary().await
}

/// The API's router for `state`, as the server mounts it.
pub fn router(state: AppState) -> axum::Router {
    api::routes::api(state.clone())
        .with_state(state)
        .split_for_parts()
        .0
}

/// A test server for `router`; cookies are passed by each request.
pub fn server(router: axum::Router) -> App {
    Arc::new(TestServer::builder().mock_transport().build(router))
}

/// A test server that keeps the cookies it is given between requests, like a browser.
pub fn browser(router: axum::Router) -> TestServer {
    TestServer::builder()
        .mock_transport()
        .save_cookies()
        .build(router)
}

/// State for tests: the limits on sign-in attempts are out of the way (`tests/it/limits.rs` checks them).
pub fn test_state(db: sea_orm::DatabaseConnection) -> AppState {
    AppState::new(db).with_limits(api::limits::Limits::generous())
}

pub async fn app() -> App {
    app_with_db().await.0
}

/// The app with a small upload limit, for testing size checks.
pub async fn app_with_upload_limit(max_bytes: u64) -> (App, sea_orm::DatabaseConnection) {
    let db = database().await;
    let root = std::env::temp_dir().join(format!("woodmouse-test-{}", uuid::Uuid::new_v4()));
    let files = api::storage::FileStores::new(
        std::sync::Arc::new(api::storage::LocalDisk::new(root)),
        max_bytes,
    );
    (
        server(router(
            AppState::with_files(db.clone(), files).with_limits(api::limits::Limits::generous()),
        )),
        db,
    )
}

/// The app and a handle on its database (the clone shares the pool), to seed users and sessions directly.
pub async fn app_with_db() -> (App, sea_orm::DatabaseConnection) {
    let db = database().await;
    (server(router(test_state(db.clone()))), db)
}

/// The app and its state, to hold the write turn (`state.writer()`) and so line up concurrent requests.
pub async fn app_with_state() -> (App, AppState) {
    let state = test_state(database().await);
    (server(router(state.clone())), state)
}

/// What the test requests add: a person's session cookie, and a JSON body given as text.
pub trait RequestExt {
    /// Send `cookie` (`name=value`), if any.
    fn session(self, cookie: Option<&str>) -> Self;
    /// A JSON body, as written.
    fn json_text(self, json: &str) -> Self;
}

impl RequestExt for TestRequest {
    fn session(self, cookie: Option<&str>) -> Self {
        match cookie {
            Some(cookie) => self.add_header(
                axum::http::header::COOKIE,
                HeaderValue::from_str(cookie).expect("a cookie header"),
            ),
            None => self,
        }
    }

    fn json_text(self, json: &str) -> Self {
        self.content_type("application/json")
            .bytes(json.to_string().into())
    }
}

/// The first `Set-Cookie` of the answer, as `name=value`.
pub fn cookie_of(res: &TestResponse) -> String {
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

/// Every `Set-Cookie` of the answer, whole.
pub fn set_cookies(res: &TestResponse) -> Vec<String> {
    res.headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().to_string())
        .collect()
}

/// The answer's body as JSON.
pub fn json_of(res: &TestResponse) -> serde_json::Value {
    serde_json::from_str(&res.text()).expect("valid json")
}

/// Bootstrap the first (super admin) user and return the session cookie.
pub async fn bootstrap_admin(app: &TestServer) -> String {
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"admin@home.io","name":"Admin","password":"correct horse"}"#)
        .await;
    if res.status_code() != StatusCode::CREATED {
        panic!("bootstrap: {} {}", res.status_code(), res.text());
    }
    cookie_of(&res)
}

/// Create a workspace and return its id.
pub async fn create_workspace(app: &TestServer, cookie: &str, name: &str) -> uuid::Uuid {
    let res = app
        .post("/api/workspaces")
        .json_text(&format!(r#"{{"name":"{name}"}}"#))
        .session(Some(cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    json_of(&res)
        .get("id")
        .expect("id in response")
        .as_str()
        .expect("id as string")
        .parse()
        .expect("valid uuid")
}

/// A multipart upload with one `file` part and optionally a `task_id` text part.
pub fn upload(
    app: &TestServer,
    uri: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
    task_id: Option<&str>,
    cookie: Option<&str>,
) -> TestRequest {
    let mut form = MultipartForm::new();
    if let Some(task_id) = task_id {
        form = form.add_text("task_id", task_id.to_string());
    }
    let file = Part::bytes(bytes.to_vec())
        .file_name(filename)
        .mime_type(content_type);
    app.post(uri)
        .multipart(form.add_part("file", file))
        .session(cookie)
}
