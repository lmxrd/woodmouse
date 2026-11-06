//! API keys: made with a session, used as `Authorization: Bearer wm_...`, capped by their scope and by
//! the person's own role, and never able to manage the account.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, sea_query::Expr};
use serde_json::Value;

use api::state::AppState;

use crate::common::*;

async fn json(res: axum::http::Response<Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

/// A request with a bearer token instead of the session cookie.
fn with_key(method: &str, uri: &str, token: &str, body: Option<&str>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap()
}

/// Make a key with the session; its id and token.
async fn create_key(app: &axum::Router, session: &str, scope: &str) -> (String, String) {
    let res = request(
        app,
        post_json(
            "/api/auth/api-keys",
            &format!(r#"{{"name":"script","scope":"{scope}"}}"#),
            Some(session),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = json(res).await;
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("wm_"), "{token}");
    assert_eq!(created["key"]["prefix"], token[..11]);
    (created["key"]["id"].as_str().unwrap().to_string(), token)
}

async fn status(app: &axum::Router, req: Request<Body>) -> StatusCode {
    request(app, req).await.status()
}

/// An ordinary user with a password, and their session.
async fn add_user(app: &axum::Router, state: &AppState, email: &str) -> (uuid::Uuid, String) {
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
    let res = request(
        app,
        post_json(
            "/api/auth/login",
            &format!(r#"{{"email":"{email}","password":"a fine password"}}"#),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    (id, cookie_of(&res))
}

#[tokio::test]
async fn a_read_key_only_reads() {
    let (app, _) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let workspace = create_workspace(&app, &admin, "Home").await;
    let (_, token) = create_key(&app, &admin, "read").await;

    assert_eq!(
        status(&app, with_key("GET", "/api/workspaces", &token, None)).await,
        StatusCode::OK
    );
    let uri = format!("/api/workspaces/{workspace}");
    assert_eq!(
        status(&app, with_key("GET", &uri, &token, None)).await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &app,
            with_key(
                "POST",
                "/api/workspaces",
                &token,
                Some(r#"{"name":"Other"}"#)
            )
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &app,
            with_key("PATCH", &uri, &token, Some(r#"{"name":"Renamed"}"#))
        )
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
        status(
            &app,
            with_key(
                "POST",
                "/api/workspaces",
                &token,
                Some(r#"{"name":"Other"}"#)
            )
        )
        .await,
        StatusCode::CREATED
    );
    // No site admin, though its owner is the super admin.
    assert_eq!(
        status(&app, with_key("GET", "/api/admin/users", &token, None)).await,
        StatusCode::FORBIDDEN
    );
    // And no workspace admin: the role there is capped at write.
    let uri = format!("/api/workspaces/{workspace}");
    assert_eq!(
        status(
            &app,
            with_key("PATCH", &uri, &token, Some(r#"{"name":"Renamed"}"#))
        )
        .await,
        StatusCode::NOT_FOUND
    );
    // An admin key does what its owner may.
    let (_, admin_key) = create_key(&app, &admin, "admin").await;
    assert_eq!(
        status(&app, with_key("GET", "/api/admin/users", &admin_key, None)).await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &app,
            with_key("PATCH", &uri, &admin_key, Some(r#"{"name":"Renamed"}"#))
        )
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
    let res = request(&app, with_key("GET", "/api/auth/me", &token, None)).await;
    assert_eq!(res.status(), StatusCode::OK);
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
            status(&app, with_key(method, &uri, &token, body)).await,
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
    let res = request(
        &app,
        post_json(
            "/api/auth/api-keys",
            r#"{"name":"x","scope":"admin"}"#,
            Some(&sara),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    create_key(&app, &sara, "write").await;

    // Bad names and expiries.
    for body in [
        r#"{"name":"  ","scope":"read"}"#,
        r#"{"name":"x","scope":"read","expires_in_days":0}"#,
    ] {
        let res = request(&app, post_json("/api/auth/api-keys", body, Some(&sara))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn revoked_expired_and_disabled_keys_stop_working() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (sara_id, sara) = add_user(&app, &state, "sara@home.io").await;

    // Revoked.
    let (id, token) = create_key(&app, &admin, "read").await;
    let me = || with_key("GET", "/api/auth/me", &token, None);
    assert_eq!(status(&app, me()).await, StatusCode::OK);
    let res = request(
        &app,
        delete(&format!("/api/auth/api-keys/{id}"), Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(status(&app, me()).await, StatusCode::UNAUTHORIZED);
    let listed = json(request(&app, get("/api/auth/api-keys", Some(&admin))).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 0);

    // Expired.
    let res = request(
        &app,
        post_json(
            "/api/auth/api-keys",
            r#"{"name":"soon","scope":"read","expires_in_days":1}"#,
            Some(&admin),
        ),
    )
    .await;
    let created = json(res).await;
    assert!(created["key"]["expires_at"].is_string());
    let token = created["token"].as_str().unwrap().to_string();
    assert_eq!(
        status(&app, with_key("GET", "/api/auth/me", &token, None)).await,
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
        status(&app, with_key("GET", "/api/auth/me", &token, None)).await,
        StatusCode::UNAUTHORIZED
    );

    // A disabled person's key.
    let (_, token) = create_key(&app, &sara, "read").await;
    assert_eq!(
        status(&app, with_key("GET", "/api/auth/me", &token, None)).await,
        StatusCode::OK
    );
    let res = request(
        &app,
        patch_json(
            &format!("/api/admin/users/{sara_id}"),
            r#"{"active":false}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        status(&app, with_key("GET", "/api/auth/me", &token, None)).await,
        StatusCode::UNAUTHORIZED
    );

    // Unknown tokens, and someone else's key id.
    assert_eq!(
        status(&app, with_key("GET", "/api/auth/me", "wm_nothing", None)).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn keys_are_listed_without_their_token_and_audited() {
    let (app, state) = app_with_state().await;
    let admin = bootstrap_admin(&app).await;
    let (id, token) = create_key(&app, &admin, "read").await;

    let listed = json(request(&app, get("/api/auth/api-keys", Some(&admin))).await).await;
    let key = &listed.as_array().unwrap()[0];
    assert_eq!(key["id"], id);
    assert_eq!(key["scope"], "read");
    assert!(key["last_used_at"].is_null());
    assert!(key.get("token").is_none());
    assert!(!listed.to_string().contains(&token));

    request(&app, with_key("GET", "/api/workspaces", &token, None)).await;
    let listed = json(request(&app, get("/api/auth/api-keys", Some(&admin))).await).await;
    assert!(listed[0]["last_used_at"].is_string());

    // Someone else cannot revoke it.
    let (_, sara) = add_user(&app, &state, "sara@home.io").await;
    let res = request(
        &app,
        delete(&format!("/api/auth/api-keys/{id}"), Some(&sara)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        delete(&format!("/api/auth/api-keys/{id}"), Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let audit = json(request(&app, get("/api/admin/audit?limit=500", Some(&admin))).await).await;
    let actions: Vec<&str> = audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"api_key_created"), "{actions:?}");
    assert!(actions.contains(&"api_key_revoked"), "{actions:?}");
}
