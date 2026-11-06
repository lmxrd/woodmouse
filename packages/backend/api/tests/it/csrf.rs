//! Cross-site request forgery: Fetch Metadata and `Origin` on everything that changes something.

use axum::body::Body;
use axum::http::{Request, StatusCode};

use crate::common::*;

/// A POST of `json` with the session cookie and the given headers.
fn post_with(
    uri: &str,
    json: &str,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("host", "tasks.example.com")
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(json.to_string())).unwrap()
}

const NEW_WORKSPACE: &str = r#"{"name":"Elsewhere"}"#;

#[tokio::test]
async fn a_cross_site_request_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    for site in ["cross-site", "same-site"] {
        let res = request(
            &app,
            post_with(
                "/api/workspaces",
                NEW_WORKSPACE,
                Some(&admin),
                &[("sec-fetch-site", site)],
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{site}");
    }
    // Signing in from another site too (login CSRF).
    let res = request(
        &app,
        post_with(
            "/api/auth/login",
            r#"{"email":"admin@home.io","password":"correct horse"}"#,
            None,
            &[("sec-fetch-site", "cross-site")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    // Reading is not affected.
    let res = request(
        &app,
        Request::builder()
            .uri("/api/workspaces")
            .header("cookie", &admin)
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_mismatched_origin_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = request(
        &app,
        post_with(
            "/api/workspaces",
            NEW_WORKSPACE,
            Some(&admin),
            &[("origin", "https://evil.example")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = request(
        &app,
        post_with(
            "/api/workspaces",
            NEW_WORKSPACE,
            Some(&admin),
            &[("origin", "https://tasks.example.com")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn same_origin_requests_work() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = request(
        &app,
        post_with(
            "/api/workspaces",
            NEW_WORKSPACE,
            Some(&admin),
            &[
                ("sec-fetch-site", "same-origin"),
                ("origin", "https://tasks.example.com"),
            ],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_upload_from_a_sibling_site_is_refused() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let mut req = upload(
        &format!("/api/workspaces/{ws}/files"),
        "note.txt",
        "text/plain",
        b"hello",
        None,
        Some(&admin),
    );
    req.headers_mut()
        .insert("sec-fetch-site", "same-site".parse().unwrap());
    assert_eq!(request(&app, req).await.status(), StatusCode::FORBIDDEN);
    // The same upload from the app itself.
    let mut req = upload(
        &format!("/api/workspaces/{ws}/files"),
        "note.txt",
        "text/plain",
        b"hello",
        None,
        Some(&admin),
    );
    req.headers_mut()
        .insert("sec-fetch-site", "same-origin".parse().unwrap());
    assert_eq!(request(&app, req).await.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_api_key_works_whatever_the_headers() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = request(
        &app,
        post_json(
            "/api/auth/api-keys",
            r#"{"name":"script","scope":"write"}"#,
            Some(&admin),
        ),
    )
    .await;
    let created: serde_json::Value =
        serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    let token = created["token"].as_str().unwrap();
    let res = request(
        &app,
        post_with(
            "/api/workspaces",
            NEW_WORKSPACE,
            None,
            &[
                ("authorization", &format!("Bearer {token}")),
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://elsewhere.example"),
            ],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}
