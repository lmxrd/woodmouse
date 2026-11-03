//! Profile, display preferences, and password changes.

use axum::http::StatusCode;
use serde_json::Value;

use crate::common::*;

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

/// Someone without a password (signing in only through single sign-on): a session of theirs, made directly.
async fn passwordless_session(db: &sea_orm::DatabaseConnection, email: &str) -> String {
    use api::db::prelude::*;
    let user = entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set(String::new()),
        password_hash: Set(None),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    let (session, _) = api::auth::create_session(db, user.id, false).await.unwrap();
    format!("{}={}", api::auth::SESSION_COOKIE, session.token)
}

#[tokio::test]
async fn an_account_without_a_password_cannot_log_in_with_one() {
    let (app, db) = app_with_db().await;
    bootstrap_admin(&app).await;
    passwordless_session(&db, "sso@home.io").await;
    // The same answer as a wrong password: no way to tell the account has none.
    for password in ["", "anything at all"] {
        let res = request(
            &app,
            post_json(
                "/api/auth/login",
                &format!(r#"{{"email":"sso@home.io","password":"{password}"}}"#),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn a_first_password_needs_no_current_one_and_then_works() {
    let (app, db) = app_with_db().await;
    bootstrap_admin(&app).await;
    let cookie = passwordless_session(&db, "sso@home.io").await;
    let res = request(
        &app,
        post_json(
            "/api/auth/password",
            r#"{"new_password":"my first password"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        login(&app, "sso@home.io", "my first password").await.0,
        StatusCode::OK
    );
    // Now that there is one, changing it needs it.
    let res = request(
        &app,
        post_json(
            "/api/auth/password",
            r#"{"new_password":"another password"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// Two requests setting a first password at once (neither needs a current one): only one may win, even
/// when both checked before either wrote.
#[tokio::test]
async fn only_one_of_two_first_passwords_at_once_is_set() {
    let (app, state) = app_with_state().await;
    bootstrap_admin(&app).await;
    let cookie = passwordless_session(state.db(), "sso@home.io").await;
    let set = |password: &'static str| {
        let (app, cookie) = (app.clone(), cookie.clone());
        tokio::spawn(async move {
            request(
                &app,
                post_json(
                    "/api/auth/password",
                    &format!(r#"{{"new_password":"{password}"}}"#),
                    Some(&cookie),
                ),
            )
            .await
            .status()
        })
    };
    // Both pass the check and wait for the write turn held here.
    let turn = state.writer().await.unwrap();
    let (a, b) = (set("first password a"), set("first password b"));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    drop(turn);
    let mut statuses = [a.await.unwrap(), b.await.unwrap()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::NO_CONTENT, StatusCode::CONFLICT]);
}
