//! API keys: made with a session, used as `Authorization: Bearer wm_...`, capped by their scope and by
//! the person's own role, and never able to manage the account.

use axum::http::StatusCode;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, sea_query::Expr};
use serde_json::Value;

use api::state::AppState;

use crate::common::*;

async fn json(res: TestResponse) -> Value {
    serde_json::from_str(&res.text()).expect("valid json")
}

/// A request with a bearer token instead of the session cookie.
fn with_key(
    app: &TestServer,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<&str>,
) -> TestRequest {
    let request = app
        .method(method.parse().expect("a method"), uri)
        .authorization_bearer(token);
    match body {
        Some(body) => request.json_text(body),
        None => request,
    }
}

/// Make a key with the session; its id and token.
async fn create_key(app: &TestServer, session: &str, scope: &str) -> (String, String) {
    let res = app
        .post("/api/auth/api-keys")
        .json_text(&format!(r#"{{"name":"script","scope":"{scope}"}}"#))
        .session(Some(session))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let created = json(res).await;
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("wm_"), "{token}");
    assert_eq!(created["key"]["prefix"], token[..11]);
    (created["key"]["id"].as_str().unwrap().to_string(), token)
}

async fn status(req: TestRequest) -> StatusCode {
    req.await.status_code()
}

/// An ordinary user with a password, and their session.
async fn add_user(app: &TestServer, state: &AppState, email: &str) -> (uuid::Uuid, String) {
    let id = entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set(String::new()),
        password_hash: Set(Some(api::auth::hash_password("a fine password").unwrap())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(state.db())
    .await
    .unwrap()
    .id;
    let res = app
        .post("/api/auth/login")
        .json_text(&format!(
            r#"{{"email":"{email}","password":"a fine password"}}"#
        ))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    (id, cookie_of(&res))
}

#[tokio::test]
async fn a_read_key_only_reads() {
    let (app, _) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let workspace = create_workspace(&app, &admin, "Home").await;
    let (_, token) = create_key(&app, &admin, "read").await;

    assert_eq!(
        status(with_key(&app, "GET", "/api/workspaces", &token, None)).await,
        StatusCode::OK
    );
    let uri = format!("/api/workspaces/{workspace}");
    assert_eq!(
        status(with_key(&app, "GET", &uri, &token, None)).await,
        StatusCode::OK
    );
    assert_eq!(
        status(with_key(
            &app,
            "POST",
            "/api/workspaces",
            &token,
            Some(r#"{"name":"Other"}"#)
        ))
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(with_key(
            &app,
            "PATCH",
            &uri,
            &token,
            Some(r#"{"name":"Renamed"}"#)
        ))
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_write_key_changes_things_but_never_administers() {
    let (app, _) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let workspace = create_workspace(&app, &admin, "Home").await;
    let (_, token) = create_key(&app, &admin, "write").await;

    assert_eq!(
        status(with_key(
            &app,
            "POST",
            "/api/workspaces",
            &token,
            Some(r#"{"name":"Other"}"#)
        ))
        .await,
        StatusCode::CREATED
    );
    // No site admin, though its owner is the super admin.
    assert_eq!(
        status(with_key(&app, "GET", "/api/admin/users", &token, None)).await,
        StatusCode::FORBIDDEN
    );
    // And no workspace admin: the role there is capped at write.
    let uri = format!("/api/workspaces/{workspace}");
    assert_eq!(
        status(with_key(
            &app,
            "PATCH",
            &uri,
            &token,
            Some(r#"{"name":"Renamed"}"#)
        ))
        .await,
        StatusCode::NOT_FOUND
    );
    // An admin key does what its owner may.
    let (_, admin_key) = create_key(&app, &admin, "admin").await;
    assert_eq!(
        status(with_key(&app, "GET", "/api/admin/users", &admin_key, None)).await,
        StatusCode::OK
    );
    assert_eq!(
        status(with_key(
            &app,
            "PATCH",
            &uri,
            &admin_key,
            Some(r#"{"name":"Renamed"}"#)
        ))
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_key_never_manages_the_account() {
    let (app, _) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (id, token) = create_key(&app, &admin, "admin").await;

    // Reading who it is works.
    let res = with_key(&app, "GET", "/api/auth/me", &token, None).await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(json(res).await["email"], "admin@home.io");

    for (method, uri, body) in [
        ("GET", "/api/auth/api-keys".to_string(), None),
        (
            "POST",
            "/api/auth/api-keys".to_string(),
            Some(r#"{"name":"more","scope":"admin"}"#),
        ),
        ("DELETE", format!("/api/auth/api-keys/{id}"), None),
        ("POST", "/api/auth/totp/setup".to_string(), Some("{}")),
        (
            "POST",
            "/api/auth/password".to_string(),
            Some(r#"{"current_password":"correct horse","new_password":"something else"}"#),
        ),
        (
            "PATCH",
            "/api/auth/me".to_string(),
            Some(r#"{"email":"taken@home.io"}"#),
        ),
        ("POST", "/api/auth/logout".to_string(), None),
    ] {
        assert_eq!(
            status(with_key(&app, method, &uri, &token, body)).await,
            StatusCode::FORBIDDEN,
            "{method} {uri}"
        );
    }
}

#[tokio::test]
async fn only_an_admin_makes_an_admin_key() {
    let (app, state) = app_with_state().await;
    bootstrap_admin(&app).await;
    let (_, sara) = add_user(&app, &state, "sara@home.io").await;
    let res = app
        .post("/api/auth/api-keys")
        .json_text(r#"{"name":"x","scope":"admin"}"#)
        .session(Some(&sara))
        .await;
    assert_eq!(res.status_code(), StatusCode::FORBIDDEN);
    create_key(&app, &sara, "write").await;

    // Bad names and expiries.
    for body in [
        r#"{"name":"  ","scope":"read"}"#,
        r#"{"name":"x","scope":"read","expires_in_days":0}"#,
    ] {
        let res = app
            .post("/api/auth/api-keys")
            .json_text(body)
            .session(Some(&sara))
            .await;
        assert_eq!(res.status_code(), StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn revoked_expired_and_disabled_keys_stop_working() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (sara_id, sara) = add_user(&app, &state, "sara@home.io").await;

    // Revoked.
    let (id, token) = create_key(&app, &admin, "read").await;
    let me = || with_key(&app, "GET", "/api/auth/me", &token, None);
    assert_eq!(status(me()).await, StatusCode::OK);
    let res = app
        .delete(&format!("/api/auth/api-keys/{id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    assert_eq!(status(me()).await, StatusCode::UNAUTHORIZED);
    let listed = json(app.get("/api/auth/api-keys").session(Some(&admin)).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 0);

    // Expired.
    let res = app
        .post("/api/auth/api-keys")
        .json_text(r#"{"name":"soon","scope":"read","expires_in_days":1}"#)
        .session(Some(&admin))
        .await;
    let created = json(res).await;
    assert!(created["key"]["expires_at"].is_string());
    let token = created["token"].as_str().unwrap().to_string();
    assert_eq!(
        status(with_key(&app, "GET", "/api/auth/me", &token, None)).await,
        StatusCode::OK
    );
    entity::api_keys::Entity::update_many()
        .col_expr(
            entity::api_keys::Column::ExpiresAt,
            Expr::value(api::time::add_seconds(api::time::now(), -1)),
        )
        .exec(state.db())
        .await
        .unwrap();
    assert_eq!(
        status(with_key(&app, "GET", "/api/auth/me", &token, None)).await,
        StatusCode::UNAUTHORIZED
    );

    // A disabled person's key.
    let (_, token) = create_key(&app, &sara, "read").await;
    assert_eq!(
        status(with_key(&app, "GET", "/api/auth/me", &token, None)).await,
        StatusCode::OK
    );
    let res = app
        .patch(&format!("/api/admin/users/{sara_id}"))
        .json_text(r#"{"active":false}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(
        status(with_key(&app, "GET", "/api/auth/me", &token, None)).await,
        StatusCode::UNAUTHORIZED
    );

    // Unknown tokens, and someone else's key id.
    assert_eq!(
        status(with_key(&app, "GET", "/api/auth/me", "wm_nothing", None)).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn keys_are_listed_without_their_token_and_audited() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (id, token) = create_key(&app, &admin, "read").await;

    let listed = json(app.get("/api/auth/api-keys").session(Some(&admin)).await).await;
    let key = &listed.as_array().unwrap()[0];
    assert_eq!(key["id"], id);
    assert_eq!(key["scope"], "read");
    assert!(key["last_used_at"].is_null());
    assert!(key.get("token").is_none());
    assert!(!listed.to_string().contains(&token));

    with_key(&app, "GET", "/api/workspaces", &token, None).await;
    let listed = json(app.get("/api/auth/api-keys").session(Some(&admin)).await).await;
    assert!(listed[0]["last_used_at"].is_string());

    // Someone else cannot revoke it.
    let (_, sara) = add_user(&app, &state, "sara@home.io").await;
    let res = app
        .delete(&format!("/api/auth/api-keys/{id}"))
        .session(Some(&sara))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app
        .delete(&format!("/api/auth/api-keys/{id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);

    let audit = json(
        app.get("/api/admin/audit?limit=500")
            .session(Some(&admin))
            .await,
    )
    .await;
    let actions: Vec<&str> = audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"api_key_created"), "{actions:?}");
    assert!(actions.contains(&"api_key_revoked"), "{actions:?}");
}
