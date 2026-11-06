//! Cross-site request forgery: Fetch Metadata and `Origin` on everything that changes something.

use axum::http::{HeaderName, HeaderValue, StatusCode};

use crate::common::*;

/// The request with `headers` added, as a browser on `tasks.example.com` (no `PUBLIC_URL`) sends them.
fn with_headers(request: TestRequest, headers: &[(&str, &str)]) -> TestRequest {
    let mut request = request.add_header(
        axum::http::header::HOST,
        HeaderValue::from_static("tasks.example.com"),
    );
    for (name, value) in headers {
        request = request.add_header(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    request
}

/// A POST of `json` with the session cookie and the given headers.
fn post_with(
    app: &TestServer,
    uri: &str,
    json: &str,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
) -> TestRequest {
    with_headers(app.post(uri).json_text(json).session(cookie), headers)
}

const NEW_WORKSPACE: &str = r#"{"name":"Elsewhere"}"#;

#[tokio::test]
async fn a_cross_site_request_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    for site in ["cross-site", "same-site"] {
        let res = post_with(
            &app,
            "/api/workspaces",
            NEW_WORKSPACE,
            Some(&admin),
            &[("sec-fetch-site", site)],
        )
        .await;
        assert_eq!(res.status_code(), StatusCode::FORBIDDEN, "{site}");
    }
    // Signing in from another site too (login CSRF).
    let res = post_with(
        &app,
        "/api/auth/login",
        r#"{"email":"admin@home.io","password":"correct horse"}"#,
        None,
        &[("sec-fetch-site", "cross-site")],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::FORBIDDEN);
    // Reading is not affected.
    let res = with_headers(
        app.get("/api/workspaces").session(Some(&admin)),
        &[("sec-fetch-site", "cross-site")],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::OK);
}

#[tokio::test]
async fn a_mismatched_origin_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = post_with(
        &app,
        "/api/workspaces",
        NEW_WORKSPACE,
        Some(&admin),
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::FORBIDDEN);
    let res = post_with(
        &app,
        "/api/workspaces",
        NEW_WORKSPACE,
        Some(&admin),
        &[("origin", "https://tasks.example.com")],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
}

#[tokio::test]
async fn same_origin_requests_work() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = post_with(
        &app,
        "/api/workspaces",
        NEW_WORKSPACE,
        Some(&admin),
        &[
            ("sec-fetch-site", "same-origin"),
            ("origin", "https://tasks.example.com"),
        ],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_upload_from_a_sibling_site_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let uri = format!("/api/workspaces/{ws}/files");
    let file = |site: &str| {
        with_headers(
            upload(
                &app,
                &uri,
                "note.txt",
                "text/plain",
                b"hello",
                None,
                Some(&admin),
            ),
            &[("sec-fetch-site", site)],
        )
    };
    assert_eq!(file("same-site").await.status_code(), StatusCode::FORBIDDEN);
    // The same upload from the app itself.
    assert_eq!(file("same-origin").await.status_code(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_api_key_works_whatever_the_headers() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = app
        .post("/api/auth/api-keys")
        .json_text(r#"{"name":"script","scope":"write"}"#)
        .session(Some(&admin))
        .await;
    let token = json_of(&res)["token"].as_str().unwrap().to_string();
    let res = post_with(
        &app,
        "/api/workspaces",
        NEW_WORKSPACE,
        None,
        &[
            ("authorization", &format!("Bearer {token}")),
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://elsewhere.example"),
        ],
    )
    .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
}
