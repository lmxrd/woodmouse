//! Profile, display preferences, and password changes.

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

async fn login(app: &axum::Router, email: &str, password: &str) -> (StatusCode, Option<String>) {
    let res = request(
        app,
        post_json(
            "/api/auth/login",
            &format!(r#"{{"email":"{email}","password":"{password}"}}"#),
            None,
        ),
    )
    .await;
    let status = res.status();
    let cookie = (status == StatusCode::OK).then(|| cookie_of(&res));
    (status, cookie)
}

#[tokio::test]
async fn display_name_is_optional_and_preferences_default() {
    let app = app().await;
    let res = request(
        &app,
        post_json(
            "/api/auth/bootstrap",
            r#"{"email":"me@home.io","password":"a good password"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let cookie = cookie_of(&res);
    let me = json(res).await;
    assert_eq!(me["name"], "");
    assert_eq!(me["date_format"], "YYYY-MM-DD");
    assert_eq!(me["time_format"], "24h");

    // Edit everything, then clear the name again.
    let res = request(
        &app,
        patch_json(
            "/api/auth/me",
            r#"{"name":"  Halvard ","date_format":"DD.MM.YYYY","time_format":"12h"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let me = json(res).await;
    assert_eq!(me["name"], "Halvard");
    assert_eq!(me["date_format"], "DD.MM.YYYY");
    assert_eq!(me["time_format"], "12h");
    let again = json(request(&app, get("/api/auth/me", Some(&cookie))).await).await;
    assert_eq!(again["date_format"], "DD.MM.YYYY", "persisted");

    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"name":""}"#, Some(&cookie)),
    )
    .await;
    assert_eq!(json(res).await["name"], "");

    // Unknown formats are rejected.
    for body in [
        r#"{"date_format":"whenever"}"#,
        r#"{"time_format":"25h"}"#,
        r#"{"email":"nope"}"#,
    ] {
        let res = request(&app, patch_json("/api/auth/me", body, Some(&cookie))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn email_changes_and_conflicts() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/members"),
            r#"{"email":"sara@home.io","role":"write"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::CREATED,
        "a name is not required to invite"
    );

    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"email":"SARA@home.io"}"#, Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"email":"New@Home.io"}"#, Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["email"], "new@home.io");
    // Setting your own email again is not a conflict.
    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"email":"new@home.io"}"#, Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn changing_the_password_signs_out_other_sessions() {
    let app = app().await;
    let first = bootstrap_admin(&app).await; // password "correct horse"
    let (status, second) = login(&app, "admin@home.io", "correct horse").await;
    assert_eq!(status, StatusCode::OK);
    let second = second.unwrap();

    let change = |cookie: String, current: &str, new: &str| {
        let app = app.clone();
        let body = format!(r#"{{"current_password":"{current}","new_password":"{new}"}}"#);
        async move {
            request(&app, post_json("/api/auth/password", &body, Some(&cookie)))
                .await
                .status()
        }
    };
    assert_eq!(
        change(first.clone(), "wrong password", "a new password").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        change(first.clone(), "correct horse", "short").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        change(first.clone(), "correct horse", "a new password").await,
        StatusCode::NO_CONTENT
    );

    // This session stays; the other one is gone; only the new password logs in.
    assert_eq!(
        request(&app, get("/api/auth/me", Some(&first)))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, get("/api/auth/me", Some(&second)))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "admin@home.io", "correct horse").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "admin@home.io", "a new password").await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn week_start_preference() {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let me = json(request(&app, get("/api/auth/me", Some(&cookie))).await).await;
    assert_eq!(me["week_start"], "monday");
    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"week_start":"sunday"}"#, Some(&cookie)),
    )
    .await;
    assert_eq!(json(res).await["week_start"], "sunday");
    assert_eq!(
        json(request(&app, get("/api/auth/me", Some(&cookie))).await).await["week_start"],
        "sunday"
    );
    let res = request(
        &app,
        patch_json("/api/auth/me", r#"{"week_start":"friday"}"#, Some(&cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
