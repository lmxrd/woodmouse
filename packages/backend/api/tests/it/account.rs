//! Profile, display preferences, and password changes.

use axum::http::StatusCode;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde_json::Value;

use crate::common::*;

async fn json(res: TestResponse) -> Value {
    serde_json::from_str(&res.text()).expect("valid json")
}

async fn login(app: &TestServer, email: &str, password: &str) -> (StatusCode, Option<String>) {
    let res = app
        .post("/api/auth/login")
        .json_text(&format!(r#"{{"email":"{email}","password":"{password}"}}"#))
        .await;
    let status = res.status_code();
    let cookie = (status == StatusCode::OK).then(|| cookie_of(&res));
    (status, cookie)
}

#[tokio::test]
async fn display_name_is_optional_and_preferences_default() {
    let app = app().await;
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"me@home.io","password":"a good password"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let cookie = cookie_of(&res);
    let me = json(res).await;
    assert_eq!(me["name"], "");
    assert_eq!(me["date_format"], "YYYY-MM-DD");
    assert_eq!(me["time_format"], "24h");

    // Edit everything, then clear the name again.
    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"name":"  Halvard ","date_format":"DD.MM.YYYY","time_format":"12h"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let me = json(res).await;
    assert_eq!(me["name"], "Halvard");
    assert_eq!(me["date_format"], "DD.MM.YYYY");
    assert_eq!(me["time_format"], "12h");
    let again = json(app.get("/api/auth/me").session(Some(&cookie)).await).await;
    assert_eq!(again["date_format"], "DD.MM.YYYY", "persisted");

    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"name":""}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(json(res).await["name"], "");

    // Unknown formats are rejected.
    for body in [
        r#"{"date_format":"whenever"}"#,
        r#"{"time_format":"25h"}"#,
        r#"{"email":"nope"}"#,
    ] {
        let res = app
            .patch("/api/auth/me")
            .json_text(body)
            .session(Some(&cookie))
            .await;
        assert_eq!(res.status_code(), StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn email_changes_and_conflicts() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let res = app
        .post(&format!("/api/workspaces/{ws}/members"))
        .json_text(r#"{"email":"sara@home.io","role":"write"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(
        res.status_code(),
        StatusCode::CREATED,
        "a name is not required to invite"
    );

    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"email":"SARA@home.io"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);
    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"email":"New@Home.io"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(json(res).await["email"], "new@home.io");
    // Setting your own email again is not a conflict.
    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"email":"new@home.io"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
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
            app.post("/api/auth/password")
                .json_text(&body)
                .session(Some(&cookie))
                .await
                .status_code()
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
        app.get("/api/auth/me")
            .session(Some(&first))
            .await
            .status_code(),
        StatusCode::OK
    );
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&second))
            .await
            .status_code(),
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
    let me = json(app.get("/api/auth/me").session(Some(&cookie)).await).await;
    assert_eq!(me["week_start"], "monday");
    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"week_start":"sunday"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(json(res).await["week_start"], "sunday");
    assert_eq!(
        json(app.get("/api/auth/me").session(Some(&cookie)).await).await["week_start"],
        "sunday"
    );
    let res = app
        .patch("/api/auth/me")
        .json_text(r#"{"week_start":"friday"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);
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
    let session = api::sessions::start_session(db, user.id).await.unwrap();
    format!("{}={}", api::auth::SESSION_COOKIE, session)
}

#[tokio::test]
async fn an_account_without_a_password_cannot_log_in_with_one() {
    let (app, db) = app_with_db().await;
    bootstrap_admin(&app).await;
    passwordless_session(&db, "sso@home.io").await;
    // The same answer as a wrong password: no way to tell the account has none.
    for password in ["", "anything at all"] {
        let res = app
            .post("/api/auth/login")
            .json_text(&format!(
                r#"{{"email":"sso@home.io","password":"{password}"}}"#
            ))
            .await;
        assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn a_first_password_needs_no_current_one_and_then_works() {
    let (app, db) = app_with_db().await;
    bootstrap_admin(&app).await;
    let cookie = passwordless_session(&db, "sso@home.io").await;
    let res = app
        .post("/api/auth/password")
        .json_text(r#"{"new_password":"my first password"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    assert_eq!(
        login(&app, "sso@home.io", "my first password").await.0,
        StatusCode::OK
    );
    // Now that there is one, changing it needs it.
    let res = app
        .post("/api/auth/password")
        .json_text(r#"{"new_password":"another password"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
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
            app.post("/api/auth/password")
                .json_text(&format!(r#"{{"new_password":"{password}"}}"#))
                .session(Some(&cookie))
                .await
                .status_code()
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

/// The app as served over https (`PUBLIC_URL` with `https://`), or not.
async fn app_served(secure: bool) -> App {
    let mut state = api::state::AppState::new(api::db::temporary().await)
        .with_limits(api::limits::Limits::generous());
    state.secure_cookies = secure;
    server(router(state))
}

fn set_cookies(res: &TestResponse) -> Vec<String> {
    res.headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().to_string())
        .collect()
}

fn has_parts(cookie: &str, parts: &[&str]) {
    for part in parts {
        assert!(format!("{cookie};").contains(part), "{cookie} lacks {part}");
    }
}

/// Over https the session cookie is `__Host-` prefixed (a sibling subdomain cannot plant one), only that
/// name is read, and logout clears it with the same attributes.
#[tokio::test]
async fn over_https_the_session_cookie_is_host_prefixed() {
    let app = app_served(true).await;
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"admin@home.io","name":"","password":"correct horse"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let cookies = set_cookies(&res);
    let session = cookies
        .iter()
        .find(|c| c.starts_with("__Host-woodmouse_session="))
        .expect("the prefixed session cookie");
    has_parts(
        session,
        &["Secure", "HttpOnly", "SameSite=Strict", "Path=/;"],
    );
    assert!(!session.contains("Domain"), "{session}");
    // A plain one from before the prefix is let go.
    let old = cookies
        .iter()
        .find(|c| c.starts_with("woodmouse_session=;"))
        .expect("the plain cookie is cleared");
    has_parts(old, &["Max-Age=0", "Path=/;"]);

    let token = session
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    let prefixed = format!("__Host-woodmouse_session={token}");
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&prefixed))
            .await
            .status_code(),
        StatusCode::OK
    );
    // The same token under the plain name (planted by a sibling subdomain) is not read.
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&format!("woodmouse_session={token}")))
            .await
            .status_code(),
        StatusCode::UNAUTHORIZED
    );

    let res = app
        .post("/api/auth/logout")
        .json_text("")
        .session(Some(&prefixed))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let removal = set_cookies(&res)
        .into_iter()
        .find(|c| c.starts_with("__Host-woodmouse_session=;"))
        .expect("the session cookie is cleared");
    has_parts(
        &removal,
        &[
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
            "Path=/;",
            "Max-Age=0",
        ],
    );
}

#[tokio::test]
async fn over_http_the_session_cookie_is_plain() {
    let app = app_served(false).await;
    let cookie = bootstrap_admin(&app).await;
    assert!(cookie.starts_with("woodmouse_session="), "{cookie}");
    let token = cookie.split_once('=').unwrap().1;
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&format!("__Host-woodmouse_session={token}")))
            .await
            .status_code(),
        StatusCode::UNAUTHORIZED
    );
    let res = app
        .post("/api/auth/logout")
        .json_text("")
        .session(Some(&cookie))
        .await;
    let removal = &set_cookies(&res)[0];
    assert!(removal.starts_with("woodmouse_session=;"), "{removal}");
    has_parts(removal, &["Path=/;", "Max-Age=0"]);
    assert!(!removal.contains("Secure"), "{removal}");
}

async fn sign_in(app: &TestServer, password: &str) -> String {
    let (status, cookie) = login(app, "admin@home.io", password).await;
    assert_eq!(status, StatusCode::OK);
    cookie.expect("a session")
}

#[tokio::test]
async fn a_password_change_signs_out_the_other_sessions_but_not_this_one() {
    let app = app().await;
    let here = bootstrap_admin(&app).await;
    let elsewhere = sign_in(&app, "correct horse").await;
    assert_ne!(here, elsewhere);

    let res = app
        .post("/api/auth/password")
        .json_text(r#"{"current_password":"correct horse","new_password":"battery staple"}"#)
        .session(Some(&here))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    // This session signed in again with the new password; the other one is out.
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&here))
            .await
            .status_code(),
        StatusCode::OK
    );
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&elsewhere))
            .await
            .status_code(),
        StatusCode::UNAUTHORIZED
    );
    // And the new password signs in.
    sign_in(&app, "battery staple").await;
}

#[tokio::test]
async fn a_disabled_persons_session_stops() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let user = entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("sara@home.io".to_string()),
        name: Set(String::new()),
        password_hash: Set(Some(api::auth::hash_password("sara's password").unwrap())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let (status, sara) = login(&app, "sara@home.io", "sara's password").await;
    assert_eq!(status, StatusCode::OK);
    let sara = sara.unwrap();
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&sara))
            .await
            .status_code(),
        StatusCode::OK
    );
    let res = app
        .patch(&format!("/api/admin/users/{}", user.id))
        .json_text(r#"{"active":false}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&sara))
            .await
            .status_code(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_ordinary_request_does_not_write_the_session() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let res = app.get("/api/auth/me").session(Some(&admin)).await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert!(res.headers().get("set-cookie").is_none());
}

#[tokio::test]
async fn enabling_an_account_again_does_not_bring_its_sessions_back() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let user = entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("ola@home.io".to_string()),
        name: Set(String::new()),
        password_hash: Set(Some(api::auth::hash_password("ola's password").unwrap())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let (_, old) = login(&app, "ola@home.io", "ola's password").await;
    let old = old.unwrap();
    let set_active = |active: bool| {
        app.patch(&format!("/api/admin/users/{}", user.id))
            .json_text(&format!(r#"{{"active":{active}}}"#))
            .session(Some(&admin))
    };
    assert_eq!(set_active(false).await.status_code(), StatusCode::OK);
    assert_eq!(set_active(true).await.status_code(), StatusCode::OK);

    // The session from before the account was disabled stays signed out, with the same password.
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&old))
            .await
            .status_code(),
        StatusCode::UNAUTHORIZED
    );
    let (status, fresh) = login(&app, "ola@home.io", "ola's password").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        app.get("/api/auth/me")
            .session(Some(&fresh.unwrap()))
            .await
            .status_code(),
        StatusCode::OK
    );
}

/// Signed in as in a browser: the server keeps the cookies it is given, and the next requests carry them.
#[tokio::test]
async fn a_browser_stays_signed_in_until_it_signs_out() {
    let browser = browser(router(test_state(api::db::temporary().await)));
    browser
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"admin@home.io","password":"correct horse"}"#)
        .await
        .assert_status(StatusCode::CREATED);
    browser.get("/api/auth/me").await.assert_status_ok();
    browser
        .post("/api/auth/logout")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    browser
        .get("/api/auth/me")
        .await
        .assert_status_unauthorized();
    browser
        .post("/api/auth/login")
        .json_text(r#"{"email":"admin@home.io","password":"correct horse"}"#)
        .await
        .assert_status_ok();
    let me = browser.get("/api/auth/me").await;
    me.assert_status_ok();
    assert_eq!(json_of(&me)["email"], "admin@home.io");
}
