//! The sign-in limits, with the quotas in force: every attempt counts, and over a limit the answer is
//! 429 with `Retry-After` and the usual error body.

use axum::http::StatusCode;

use api::state::AppState;

use crate::common::*;

/// The app with the real limits; `trust_proxy` as `TRUST_PROXY`.
async fn limited(trust_proxy: bool) -> (App, AppState) {
    let mut state = AppState::new(api::db::temporary().await);
    state.trust_proxy = trust_proxy;
    (server(router(state.clone())), state)
}

fn login_from(
    app: &TestServer,
    email: &str,
    password: &str,
    forwarded_for: Option<&str>,
) -> TestRequest {
    let req = app
        .post("/api/auth/login")
        .json_text(&format!(r#"{{"email":"{email}","password":"{password}"}}"#));
    match forwarded_for {
        Some(address) => req.add_header(
            axum::http::HeaderName::from_static("x-forwarded-for"),
            axum::http::HeaderValue::from_str(address).unwrap(),
        ),
        None => req,
    }
}

/// A 429 with a `Retry-After` of a few seconds and the error body.
async fn assert_limited(res: TestResponse, longest: u64) {
    assert_eq!(res.status_code(), StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = res
        .headers()
        .get("retry-after")
        .expect("Retry-After")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=longest).contains(&retry), "Retry-After {retry}");
    let body: serde_json::Value = serde_json::from_str(&res.text()).unwrap();
    assert_eq!(body["error"], "Too many attempts, try again later");
}

#[tokio::test]
async fn five_sign_ins_a_minute_per_email() {
    let (app, _) = limited(false).await;
    bootstrap_admin(&app).await;
    for _ in 0..4 {
        let res = login_from(&app, "admin@home.io", "wrong", None).await;
        assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
    }
    // A right password counts too.
    let res = login_from(&app, "ADMIN@home.io", "correct horse", None).await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_limited(
        login_from(&app, "admin@home.io", "correct horse", None).await,
        60,
    )
    .await;
    // Another email is not affected.
    let res = login_from(&app, "someone@home.io", "x", None).await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn thirty_attempts_per_address_behind_a_trusted_proxy() {
    let (app, _) = limited(true).await;
    for i in 0..30 {
        let res = login_from(&app, &format!("u{i}@home.io"), "x", Some("203.0.113.7")).await;
        assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    assert_limited(
        login_from(&app, "u99@home.io", "x", Some("203.0.113.7, 10.0.0.1")).await,
        6,
    )
    .await;
    // Another address is not affected.
    let res = login_from(&app, "u99@home.io", "x", Some("203.0.113.8")).await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_trust_proxy_the_forwarded_address_is_ignored() {
    let (app, _) = limited(false).await;
    for i in 0..35 {
        let res = login_from(&app, &format!("u{i}@home.io"), "x", Some("203.0.113.7")).await;
        assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED, "attempt {i}");
    }
}

#[tokio::test]
async fn five_two_factor_codes_a_minute_per_user() {
    let (app, _) = limited(false).await;
    let admin = bootstrap_admin(&app).await;
    let res = app
        .post("/api/auth/totp/setup")
        .json_text("{}")
        .session(Some(&admin))
        .await;
    let setup: serde_json::Value = serde_json::from_str(&res.text()).unwrap();
    let secret = setup["secret"].as_str().unwrap();
    let code = api::totp::code_at(secret, api::totp::current_step()).unwrap();
    // The first code (turning it on) is one attempt.
    let res = app
        .post("/api/auth/totp/enable")
        .json_text(&format!(r#"{{"code":"{code}"}}"#))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);

    let res = login_from(&app, "admin@home.io", "correct horse", None).await;
    assert_eq!(res.status_code(), StatusCode::ACCEPTED);
    let challenge = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().split(';').next().unwrap().to_string())
        .find(|c| c.starts_with("woodmouse_2fa="))
        .unwrap();
    let wrong = api::totp::code_at(secret, api::totp::current_step() + 5).unwrap();
    let attempt = || {
        app.post("/api/auth/login/totp")
            .json_text(&format!(r#"{{"code":"{wrong}"}}"#))
            .session(Some(&challenge))
    };
    for _ in 0..4 {
        assert_eq!(attempt().await.status_code(), StatusCode::UNAUTHORIZED);
    }
    assert_limited(attempt().await, 60).await;
    // The settings' code checks share that budget.
    assert_limited(
        app.post("/api/auth/totp/disable")
            .json_text(r#"{"code":"123456"}"#)
            .session(Some(&admin))
            .await,
        60,
    )
    .await;
}

#[tokio::test]
async fn five_password_changes_a_minute_per_user() {
    let (app, _) = limited(false).await;
    let admin = bootstrap_admin(&app).await;
    let change = || {
        app.post("/api/auth/password")
            .json_text(r#"{"current_password":"wrong","new_password":"another password"}"#)
            .session(Some(&admin))
    };
    for _ in 0..5 {
        assert_eq!(change().await.status_code(), StatusCode::UNAUTHORIZED);
    }
    assert_limited(change().await, 60).await;
}
