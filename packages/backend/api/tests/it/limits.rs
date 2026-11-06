//! The sign-in limits, with the quotas in force: every attempt counts, and over a limit the answer is
//! 429 with `Retry-After` and the usual error body.

use axum::body::Body;
use axum::http::{Request, StatusCode};

use api::state::AppState;

use crate::common::*;

/// The app with the real limits; `trust_proxy` as `TRUST_PROXY`.
async fn limited(trust_proxy: bool) -> (axum::Router, AppState) {
    let mut state = AppState::new(api::db::temporary().await);
    state.trust_proxy = trust_proxy;
    (router(state.clone()), state)
}

fn login_from(email: &str, password: &str, forwarded_for: Option<&str>) -> Request<Body> {
    let mut req = post_json(
        "/api/auth/login",
        &format!(r#"{{"email":"{email}","password":"{password}"}}"#),
        None,
    );
    if let Some(address) = forwarded_for {
        req.headers_mut()
            .insert("x-forwarded-for", address.parse().unwrap());
    }
    req
}

/// A 429 with a `Retry-After` of a few seconds and the error body.
async fn assert_limited(res: axum::http::Response<Body>, longest: u64) {
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = res
        .headers()
        .get("retry-after")
        .expect("Retry-After")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=longest).contains(&retry), "Retry-After {retry}");
    let body: serde_json::Value =
        serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(body["error"], "Too many attempts, try again later");
}

#[tokio::test]
async fn five_sign_ins_a_minute_per_email() {
    let (app, _) = limited(false).await;
    bootstrap_admin(&app).await;
    for _ in 0..4 {
        let res = request(&app, login_from("admin@home.io", "wrong", None)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    // A right password counts too.
    let res = request(&app, login_from("ADMIN@home.io", "correct horse", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_limited(
        request(&app, login_from("admin@home.io", "correct horse", None)).await,
        60,
    )
    .await;
    // Another email is not affected.
    let res = request(&app, login_from("someone@home.io", "x", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn thirty_attempts_per_address_behind_a_trusted_proxy() {
    let (app, _) = limited(true).await;
    for i in 0..30 {
        let res = request(
            &app,
            login_from(&format!("u{i}@home.io"), "x", Some("203.0.113.7")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    assert_limited(
        request(
            &app,
            login_from("u99@home.io", "x", Some("203.0.113.7, 10.0.0.1")),
        )
        .await,
        6,
    )
    .await;
    // Another address is not affected.
    let res = request(&app, login_from("u99@home.io", "x", Some("203.0.113.8"))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_trust_proxy_the_forwarded_address_is_ignored() {
    let (app, _) = limited(false).await;
    for i in 0..35 {
        let res = request(
            &app,
            login_from(&format!("u{i}@home.io"), "x", Some("203.0.113.7")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "attempt {i}");
    }
}

#[tokio::test]
async fn five_two_factor_codes_a_minute_per_user() {
    let (app, _) = limited(false).await;
    let admin = bootstrap_admin(&app).await;
    let res = request(&app, post_json("/api/auth/totp/setup", "{}", Some(&admin))).await;
    let setup: serde_json::Value =
        serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    let secret = setup["secret"].as_str().unwrap();
    let code = api::totp::code_at(secret, api::totp::current_step()).unwrap();
    // The first code (turning it on) is one attempt.
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/enable",
            &format!(r#"{{"code":"{code}"}}"#),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = request(&app, login_from("admin@home.io", "correct horse", None)).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let challenge = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().split(';').next().unwrap().to_string())
        .find(|c| c.starts_with("woodmouse_2fa="))
        .unwrap();
    let wrong = api::totp::code_at(secret, api::totp::current_step() + 5).unwrap();
    let attempt = || {
        post_json(
            "/api/auth/login/totp",
            &format!(r#"{{"code":"{wrong}"}}"#),
            Some(&challenge),
        )
    };
    for _ in 0..4 {
        assert_eq!(
            request(&app, attempt()).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_limited(request(&app, attempt()).await, 60).await;
    // The settings' code checks share that budget.
    assert_limited(
        request(
            &app,
            post_json(
                "/api/auth/totp/disable",
                r#"{"code":"123456"}"#,
                Some(&admin),
            ),
        )
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
        post_json(
            "/api/auth/password",
            r#"{"current_password":"wrong","new_password":"another password"}"#,
            Some(&admin),
        )
    };
    for _ in 0..5 {
        assert_eq!(
            request(&app, change()).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_limited(request(&app, change()).await, 60).await;
}
