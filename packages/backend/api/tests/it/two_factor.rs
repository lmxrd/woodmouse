//! Two-factor sign-in (TOTP): setup, the second sign-in step, replay, recovery codes, turning it off,
//! and the super admin's reset.

use axum::body::Body;
use axum::http::StatusCode;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, sea_query::Expr};
use serde_json::Value;

use api::state::AppState;
use api::totp;

use crate::common::*;

async fn json(res: axum::http::Response<Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

fn set_cookies(res: &axum::http::Response<Body>) -> Vec<String> {
    res.headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().to_string())
        .collect()
}

/// `name=value` of the cookie called `name` that the response sets (not a removal).
fn cookie_named(res: &axum::http::Response<Body>, name: &str) -> Option<String> {
    set_cookies(res)
        .into_iter()
        .find(|c| c.starts_with(&format!("{name}=")) && !c.contains("Max-Age=0"))
        .map(|c| c.split(';').next().unwrap().to_string())
}

fn code_now(secret: &str) -> String {
    totp::code_at(secret, totp::current_step()).unwrap()
}

/// Forget which time step was used last, so the next request may use the current code again (the tests
/// run within one 30-second step).
async fn forget_used_steps(state: &AppState) {
    entity::user_totp::Entity::update_many()
        .col_expr(
            entity::user_totp::Column::LastStep,
            Expr::value(Option::<i64>::None),
        )
        .exec(state.db())
        .await
        .unwrap();
}

async fn login(app: &axum::Router, email: &str, password: &str) -> axum::http::Response<Body> {
    request(
        app,
        post_json(
            "/api/auth/login",
            &format!(r#"{{"email":"{email}","password":"{password}"}}"#),
            None,
        ),
    )
    .await
}

/// The password step for someone with two-factor on: answers 202 and the challenge cookie, no session.
async fn challenge(app: &axum::Router, email: &str, password: &str) -> String {
    let res = login(app, email, password).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert!(
        cookie_named(&res, api::auth::SESSION_COOKIE).is_none(),
        "no session before the code"
    );
    let cookie = cookie_named(&res, "woodmouse_2fa").expect("a challenge cookie");
    assert_eq!(json(res).await["totp_required"], true);
    cookie
}

async fn second_step(
    app: &axum::Router,
    challenge: &str,
    code: &str,
) -> axum::http::Response<Body> {
    request(
        app,
        post_json(
            "/api/auth/login/totp",
            &format!(r#"{{"code":"{code}"}}"#),
            Some(challenge),
        ),
    )
    .await
}

/// Turn two-factor on for the signed-in user; the secret and the recovery codes.
async fn enable(app: &axum::Router, state: &AppState, cookie: &str) -> (String, Vec<String>) {
    let res = request(app, post_json("/api/auth/totp/setup", "{}", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let setup = json(res).await;
    let secret = setup["secret"].as_str().unwrap().to_string();
    let uri = setup["otpauth_uri"].as_str().unwrap();
    assert!(uri.starts_with("otpauth://totp/Woodmouse:"), "{uri}");
    assert!(uri.contains(&format!("secret={secret}")), "{uri}");

    let res = request(
        app,
        post_json(
            "/api/auth/totp/enable",
            &format!(r#"{{"code":"{}"}}"#, code_now(&secret)),
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let codes: Vec<String> = json(res).await["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), totp::RECOVERY_CODES);
    forget_used_steps(state).await;
    (secret, codes)
}

async fn me(app: &axum::Router, cookie: &str) -> axum::http::Response<Body> {
    request(app, get("/api/auth/me", Some(cookie))).await
}

async fn audit_actions(app: &axum::Router, admin: &str) -> Vec<String> {
    let res = request(app, get("/api/admin/audit?limit=500", Some(admin))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json(res)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect()
}

/// A second, ordinary user with a password.
async fn add_user(state: &AppState, email: &str, password: &str) -> uuid::Uuid {
    entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set(String::new()),
        password_hash: Set(Some(api::auth::hash_password(password).unwrap())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(state.db())
    .await
    .unwrap()
    .id
}

const ADMIN: &str = "admin@home.io";
const PASSWORD: &str = "correct horse";

#[tokio::test]
async fn the_password_alone_is_not_enough_once_two_factor_is_on() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    assert_eq!(json(me(&app, &admin).await).await["totp_enabled"], false);
    let (secret, _) = enable(&app, &state, &admin).await;
    assert_eq!(json(me(&app, &admin).await).await["totp_enabled"], true);

    let pending = challenge(&app, ADMIN, PASSWORD).await;
    // The challenge is not a session.
    assert_eq!(me(&app, &pending).await.status(), StatusCode::UNAUTHORIZED);

    // A code from well outside the window.
    let wrong = totp::code_at(&secret, totp::current_step() + 5).unwrap();
    let res = second_step(&app, &pending, &wrong).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = second_step(&app, &pending, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::OK);
    // The challenge is cleared, the session set.
    assert!(
        set_cookies(&res)
            .iter()
            .any(|c| c.starts_with("woodmouse_2fa=;") && c.contains("Max-Age=0"))
    );
    let session = cookie_named(&res, api::auth::SESSION_COOKIE).expect("a session");
    assert_eq!(json(me(&app, &session).await).await["email"], ADMIN);

    // The same code does not work twice.
    let pending = challenge(&app, ADMIN, PASSWORD).await;
    let res = second_step(&app, &pending, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let actions = audit_actions(&app, &session).await;
    assert!(actions.contains(&"totp_enabled".to_string()));
    assert!(actions.contains(&"login_failed".to_string()));
}

#[tokio::test]
async fn the_second_step_needs_this_servers_current_challenge() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (secret, _) = enable(&app, &state, &admin).await;

    // No challenge, or one this server did not sign.
    let body = format!(r#"{{"code":"{}"}}"#, code_now(&secret));
    let res = request(&app, post_json("/api/auth/login/totp", &body, None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let forged = format!(
        r#"woodmouse_2fa={}"#,
        serde_json::to_string(&totp::Challenge::new(
            uuid::Uuid::nil(),
            totp::ChallengeMode::Code
        ))
        .unwrap()
    );
    let res = second_step(&app, &forged, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Another server's (a restart makes a new key).
    let (other, other_state) = app_with_state().await;
    let other_admin = bootstrap_admin(&other).await;
    enable(&other, &other_state, &other_admin).await;
    let foreign = challenge(&other, ADMIN, PASSWORD).await;
    let res = second_step(&app, &foreign, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_recovery_code_signs_in_once() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (_, codes) = enable(&app, &state, &admin).await;

    // Typed without dashes, in capitals: still the code.
    let typed = codes[3].replace('-', "").to_uppercase();
    let pending = challenge(&app, ADMIN, PASSWORD).await;
    let res = second_step(&app, &pending, &typed).await;
    assert_eq!(res.status(), StatusCode::OK);
    let session = cookie_named(&res, api::auth::SESSION_COOKIE).unwrap();

    let pending = challenge(&app, ADMIN, PASSWORD).await;
    let res = second_step(&app, &pending, &codes[3]).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // The others still work.
    let res = second_step(&app, &pending, &codes[4]).await;
    assert_eq!(res.status(), StatusCode::OK);

    assert!(
        audit_actions(&app, &session)
            .await
            .contains(&"recovery_code_used".to_string())
    );
}

#[tokio::test]
async fn setting_up_checks_the_first_code() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;

    // Nothing to confirm before a setup.
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/enable",
            r#"{"code":"123456"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = request(&app, post_json("/api/auth/totp/setup", "{}", Some(&admin))).await;
    let secret = json(res).await["secret"].as_str().unwrap().to_string();
    let wrong = totp::code_at(&secret, totp::current_step() + 5).unwrap();
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/enable",
            &format!(r#"{{"code":"{wrong}"}}"#),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    // Not on yet: the password alone still signs in.
    assert_eq!(login(&app, ADMIN, PASSWORD).await.status(), StatusCode::OK);
    assert_eq!(json(me(&app, &admin).await).await["totp_enabled"], false);

    // A new setup replaces the unconfirmed secret; once on, another setup is refused.
    enable(&app, &state, &admin).await;
    let res = request(&app, post_json("/api/auth/totp/setup", "{}", Some(&admin))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn turning_it_off_and_new_recovery_codes_need_a_code() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (secret, old_codes) = enable(&app, &state, &admin).await;

    // New recovery codes: the old ones stop working.
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/recovery-codes",
            &format!(r#"{{"code":"{}"}}"#, code_now(&secret)),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let new_codes = json(res).await["codes"].as_array().unwrap().clone();
    assert_eq!(new_codes.len(), totp::RECOVERY_CODES);
    let pending = challenge(&app, ADMIN, PASSWORD).await;
    let res = second_step(&app, &pending, &old_codes[0]).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Off: a wrong code is refused, a recovery code does it.
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/disable",
            r#"{"code":"aaaa-bbbb-cccc-dddd"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/disable",
            &format!(r#"{{"code":"{}"}}"#, new_codes[0].as_str().unwrap()),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(json(me(&app, &admin).await).await["totp_enabled"], false);
    assert_eq!(login(&app, ADMIN, PASSWORD).await.status(), StatusCode::OK);
    assert!(
        audit_actions(&app, &admin)
            .await
            .contains(&"totp_disabled".to_string())
    );

    // Off already.
    let res = request(
        &app,
        post_json(
            "/api/auth/totp/disable",
            r#"{"code":"123456"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_super_admin_resets_someone_who_lost_their_device() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let id = add_user(&state, "sara@home.io", "sara's password").await;
    let res = login(&app, "sara@home.io", "sara's password").await;
    let sara = cookie_of(&res);
    enable(&app, &state, &sara).await;

    let users = json(request(&app, get("/api/admin/users", Some(&admin))).await).await;
    let listed = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == id.to_string())
        .unwrap()
        .clone();
    assert_eq!(listed["totp_enabled"], true);

    // Only the super admin.
    let uri = format!("/api/admin/users/{id}/totp");
    assert_eq!(
        request(&app, delete(&uri, Some(&sara))).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, delete(&uri, Some(&admin))).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        login(&app, "sara@home.io", "sara's password")
            .await
            .status(),
        StatusCode::OK
    );
    // Nothing left to reset.
    assert_eq!(
        request(&app, delete(&uri, Some(&admin))).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        audit_actions(&app, &admin)
            .await
            .contains(&"totp_reset".to_string())
    );
}

#[tokio::test]
async fn a_disabled_account_cannot_finish_signing_in() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let id = add_user(&state, "ola@home.io", "ola's password").await;
    let ola = cookie_of(&login(&app, "ola@home.io", "ola's password").await);
    let (secret, _) = enable(&app, &state, &ola).await;

    let pending = challenge(&app, "ola@home.io", "ola's password").await;
    let res = request(
        &app,
        patch_json(
            &format!("/api/admin/users/{id}"),
            r#"{"active":false}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = second_step(&app, &pending, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// A site with `TOTP_REQUIRED`.
async fn required_app() -> (axum::Router, AppState) {
    let mut state = AppState::new(api::db::temporary().await);
    state.totp_required = true;
    (router(state.clone()), state)
}

/// The password step where two-factor is required and not set up: the enrollment challenge.
async fn enroll_challenge(app: &axum::Router, email: &str, password: &str) -> String {
    let res = login(app, email, password).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert!(cookie_named(&res, api::auth::SESSION_COOKIE).is_none());
    let cookie = cookie_named(&res, "woodmouse_2fa").expect("a challenge cookie");
    let body = json(res).await;
    assert_eq!(body["totp_setup_required"], true);
    assert_eq!(body["totp_required"], false);
    cookie
}

/// Set up two-factor from the sign-in: the session and the secret.
async fn enroll(app: &axum::Router, challenge: &str) -> (String, String) {
    let res = request(
        app,
        post_json("/api/auth/login/totp/setup", "{}", Some(challenge)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let secret = json(res).await["secret"].as_str().unwrap().to_string();
    let wrong = totp::code_at(&secret, totp::current_step() + 5).unwrap();
    let body = |code: &str| format!(r#"{{"code":"{code}"}}"#);
    let res = request(
        app,
        post_json(
            "/api/auth/login/totp/enable",
            &body(&wrong),
            Some(challenge),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = request(
        app,
        post_json(
            "/api/auth/login/totp/enable",
            &body(&code_now(&secret)),
            Some(challenge),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let session = cookie_named(&res, api::auth::SESSION_COOKIE).expect("a session");
    let enrolled = json(res).await;
    assert_eq!(
        enrolled["codes"].as_array().unwrap().len(),
        totp::RECOVERY_CODES
    );
    assert_eq!(enrolled["user"]["email"], ADMIN);
    (session, secret)
}

#[tokio::test]
async fn where_it_is_required_the_password_sign_in_sets_it_up_first() {
    let (app, state) = required_app().await;
    let first = bootstrap_admin(&app).await;
    let status = json(request(&app, get("/api/auth/status", None)).await).await;
    assert_eq!(status["totp_required"], true);

    let enrolling = enroll_challenge(&app, ADMIN, PASSWORD).await;
    // Not a code challenge, and not a session.
    assert_eq!(
        second_step(&app, &enrolling, "123456").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        me(&app, &enrolling).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // No setup without the challenge.
    let res = request(&app, post_json("/api/auth/login/totp/setup", "{}", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let (session, secret) = enroll(&app, &enrolling).await;
    assert_eq!(json(me(&app, &session).await).await["totp_enabled"], true);
    // Turning two-factor on changes the session hash: the session from before signs out.
    assert_eq!(me(&app, &first).await.status(), StatusCode::UNAUTHORIZED);

    // Next time: the code.
    forget_used_steps(&state).await;
    let pending = challenge(&app, ADMIN, PASSWORD).await;
    let res = second_step(&app, &pending, &code_now(&secret)).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn where_it_is_required_it_cannot_be_turned_off_but_can_be_reset() {
    let (app, state) = required_app().await;
    bootstrap_admin(&app).await;
    let challenge = enroll_challenge(&app, ADMIN, PASSWORD).await;
    let (session, secret) = enroll(&app, &challenge).await;
    forget_used_steps(&state).await;

    let res = request(
        &app,
        post_json(
            "/api/auth/totp/disable",
            &format!(r#"{{"code":"{}"}}"#, code_now(&secret)),
            Some(&session),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(me(&app, &session).await).await["totp_enabled"], true);

    // The super admin resets it (here their own): the next sign-in sets it up again.
    let id = json(me(&app, &session).await).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let res = request(
        &app,
        delete(&format!("/api/admin/users/{id}/totp"), Some(&session)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let challenge = enroll_challenge(&app, ADMIN, PASSWORD).await;
    enroll(&app, &challenge).await;
}

async fn replace(app: &axum::Router, session: &str, code: &str) -> axum::http::Response<Body> {
    request(
        app,
        post_json(
            "/api/auth/totp/replace",
            &format!(r#"{{"code":"{code}"}}"#),
            Some(session),
        ),
    )
    .await
}

async fn confirm(app: &axum::Router, session: &str, code: &str) -> axum::http::Response<Body> {
    request(
        app,
        post_json(
            "/api/auth/totp/replace/confirm",
            &format!(r#"{{"code":"{code}"}}"#),
            Some(session),
        ),
    )
    .await
}

/// A code from well outside the window: never right.
fn wrong_code(secret: &str) -> String {
    totp::code_at(secret, totp::current_step() + 5).unwrap()
}

/// Sign in with the password and `code`: the status of the second step.
async fn signs_in_with(app: &axum::Router, code: &str) -> StatusCode {
    let pending = challenge(app, ADMIN, PASSWORD).await;
    second_step(app, &pending, code).await.status()
}

#[tokio::test]
async fn replacing_the_app_keeps_the_old_one_until_the_new_one_is_confirmed() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;

    // Nothing to replace while it is off.
    assert_eq!(
        replace(&app, &admin, "123456").await.status(),
        StatusCode::BAD_REQUEST
    );
    let (old, codes) = enable(&app, &state, &admin).await;

    // A wrong current code is refused.
    assert_eq!(
        replace(&app, &admin, &wrong_code(&old)).await.status(),
        StatusCode::BAD_REQUEST
    );
    // No replacement started yet.
    assert_eq!(
        confirm(&app, &admin, "123456").await.status(),
        StatusCode::BAD_REQUEST
    );

    let res = replace(&app, &admin, &code_now(&old)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let setup = json(res).await;
    let new = setup["secret"].as_str().unwrap().to_string();
    assert_ne!(new, old);
    assert!(
        setup["otpauth_uri"]
            .as_str()
            .unwrap()
            .contains(&format!("secret={new}"))
    );

    // The old app still signs in; the new one does not yet.
    forget_used_steps(&state).await;
    assert_eq!(
        signs_in_with(&app, &code_now(&new)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(signs_in_with(&app, &code_now(&old)).await, StatusCode::OK);

    // A wrong new code changes nothing.
    assert_eq!(
        confirm(&app, &admin, &wrong_code(&new)).await.status(),
        StatusCode::BAD_REQUEST
    );
    forget_used_steps(&state).await;
    assert_eq!(signs_in_with(&app, &code_now(&old)).await, StatusCode::OK);

    assert_eq!(
        confirm(&app, &admin, &code_now(&new)).await.status(),
        StatusCode::NO_CONTENT
    );
    // The new app's code just confirmed it, so that step is used; the old app no longer works.
    assert_eq!(
        signs_in_with(&app, &code_now(&new)).await,
        StatusCode::UNAUTHORIZED
    );
    forget_used_steps(&state).await;
    assert_eq!(
        signs_in_with(&app, &code_now(&old)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(signs_in_with(&app, &code_now(&new)).await, StatusCode::OK);
    // The recovery codes are kept.
    assert_eq!(signs_in_with(&app, &codes[0]).await, StatusCode::OK);
    // Nothing waiting to confirm any more.
    assert_eq!(
        confirm(&app, &admin, &code_now(&new)).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(
        audit_actions(&app, &admin)
            .await
            .contains(&"totp_replaced".to_string())
    );
}

#[tokio::test]
async fn a_recovery_code_starts_a_replacement_and_is_spent() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (_, codes) = enable(&app, &state, &admin).await;
    assert_eq!(
        replace(&app, &admin, &codes[1]).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        replace(&app, &admin, &codes[1]).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_app_can_be_replaced_where_two_factor_is_required() {
    let (app, state) = required_app().await;
    bootstrap_admin(&app).await;
    let enrolling = enroll_challenge(&app, ADMIN, PASSWORD).await;
    let (session, old) = enroll(&app, &enrolling).await;
    forget_used_steps(&state).await;

    let res = replace(&app, &session, &code_now(&old)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let new = json(res).await["secret"].as_str().unwrap().to_string();
    assert_eq!(
        confirm(&app, &session, &code_now(&new)).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(json(me(&app, &session).await).await["totp_enabled"], true);
}

#[tokio::test]
async fn an_api_key_cannot_replace_the_app() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (secret, _) = enable(&app, &state, &admin).await;
    let res = request(
        &app,
        post_json(
            "/api/auth/api-keys",
            r#"{"name":"script","scope":"admin"}"#,
            Some(&admin),
        ),
    )
    .await;
    let token = json(res).await["token"].as_str().unwrap().to_string();
    for uri in ["/api/auth/totp/replace", "/api/auth/totp/replace/confirm"] {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"code":"{}"}}"#, code_now(&secret))))
            .unwrap();
        assert_eq!(
            request(&app, req).await.status(),
            StatusCode::FORBIDDEN,
            "{uri}"
        );
    }
}
