//! Integration tests: auth flow against the real router and an in-memory
//! SQLite database.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::{users, workspace_members};

use crate::common::*;

#[tokio::test]
async fn health_probes() {
    let app = app().await;
    let res = app.get("/api/health").await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("\"status\":\"ok\""), "body: {body}");
    assert!(body.contains("\"db\":\"ok\""), "body: {body}");
}

#[tokio::test]
async fn bootstrap_login_logout_flow() {
    let app = app().await;

    // Before any user exists: bootstrap required.
    let res = app.get("/api/auth/status").await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("\"bootstrap_required\":true"), "body: {body}");

    // Bootstrap creates the super admin and logs in.
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"admin@home.io","name":"Admin","password":"correct horse"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let cookie = cookie_of(&res);
    let body = res.text();
    assert!(body.contains("admin@home.io"), "body: {body}");
    assert!(body.contains("\"super_admin\":true"), "body: {body}");

    // Bootstrap is closed once a user exists.
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"x@y.io","name":"X","password":"longenough"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);

    let res = app.get("/api/auth/status").await;
    let body = res.text();
    assert!(
        body.contains("\"bootstrap_required\":false"),
        "body: {body}"
    );

    // The session cookie works.
    let res = app.get("/api/auth/me").session(Some(&cookie)).await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("admin@home.io"), "body: {body}");

    // Without a cookie: 401.
    let res = app.get("/api/auth/me").await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);

    // Wrong password: 401, same generic error as unknown email.
    let res = app
        .post("/api/auth/login")
        .json_text(r#"{"email":"admin@home.io","password":"wrong pass"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);

    let res = app
        .post("/api/auth/login")
        .json_text(r#"{"email":"nobody@home.io","password":"wrong pass"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);

    // Correct credentials: a fresh session.
    let res = app
        .post("/api/auth/login")
        .json_text(r#"{"email":"admin@home.io","password":"correct horse"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let cookie2 = cookie_of(&res);

    // Logout invalidates only the logged-out session.
    let res = app
        .post("/api/auth/logout")
        .json_text("")
        .session(Some(&cookie2))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = app.get("/api/auth/me").session(Some(&cookie2)).await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
    let res = app.get("/api/auth/me").session(Some(&cookie)).await;
    assert_eq!(res.status_code(), StatusCode::OK);
}

#[tokio::test]
async fn bootstrap_validates_input() {
    let app = app().await;

    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"nope","name":"A","password":"longenough"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"a@b.io","name":"A","password":"short"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspaces_access_is_enforced() {
    let (app, db) = app_with_db().await;

    // First user: bootstrap super admin.
    let res = app
        .post("/api/auth/bootstrap")
        .json_text(r#"{"email":"admin@home.io","name":"Admin","password":"correct horse"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let admin_cookie = cookie_of(&res);

    // Unauthenticated access is rejected.
    let res = app.get("/api/workspaces").await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);

    // A garbage session cookie is rejected by the auth layer, not just a
    // missing one — enforcement is fail-closed for every protected route.
    let res = app
        .get("/api/workspaces")
        .session(Some("woodmouse_session=definitely-not-a-real-token"))
        .await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
    let res = app
        .get("/api/auth/me")
        .session(Some("woodmouse_session=definitely-not-a-real-token"))
        .await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);

    // The admin creates a workspace.
    let res = app
        .post("/api/workspaces")
        .json_text(r#"{"name":"Home"}"#)
        .session(Some(&admin_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let body = res.text();
    let workspace_id: uuid::Uuid = serde_json::from_slice(
        &serde_json::from_str::<serde_json::Value>(&body)
            .expect("valid json")
            .get("id")
            .expect("id in response")
            .to_string()
            .into_bytes(),
    )
    .expect("uuid id");
    assert!(body.contains("Home"), "body: {body}");

    // The admin sees it, both in the list and by id.
    let res = app
        .get("/api/workspaces")
        .session(Some(&admin_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("Home"), "body: {body}");

    let res = app
        .get(&format!("/api/workspaces/{workspace_id}"))
        .session(Some(&admin_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);

    // A second, non-member user seeded directly in the database.
    let other = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("other@home.io".to_string()),
        name: Set("Other".to_string()),
        password_hash: Set(Some("not-a-login-hash".to_string())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed second user");
    let other_token = api::sessions::start_session(&db, other.id)
        .await
        .expect("seed second session");
    let other_cookie = format!("woodmouse_session={other_token}");

    // Membership filtering: the list shows no workspaces.
    let res = app
        .get("/api/workspaces")
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert_eq!(body, "[]", "no workspaces visible: {body}");

    // Direct access to the workspace: 404 (same as nonexistent — no leak).
    let res = app
        .get(&format!("/api/workspaces/{workspace_id}"))
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // A nonexistent workspace gives the same status.
    let res = app
        .get(&format!("/api/workspaces/{}", uuid::Uuid::new_v4()))
        .session(Some(&admin_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // The second user can create their own workspace and sees only that.
    let res = app
        .post("/api/workspaces")
        .json_text(r#"{"name":"Other place"}"#)
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);

    let res = app
        .get("/api/workspaces")
        .session(Some(&other_cookie))
        .await;
    let body = res.text();
    assert!(body.contains("Other place"), "body: {body}");
    assert!(!body.contains("Home"), "body: {body}");

    // Empty name is rejected.
    let res = app
        .post("/api/workspaces")
        .json_text(r#"{"name":"  "}"#)
        .session(Some(&admin_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    // The composite primary key rejects a second membership row for the
    // same (user, workspace) pair — role lookups stay deterministic.
    let dup = workspace_members::ActiveModel {
        user_id: Set(other.id),
        workspace_id: Set(workspace_id),
        role: Set(entity::Role::Read),
        ..Default::default()
    }
    .insert(&db)
    .await;
    let dup2 = workspace_members::ActiveModel {
        user_id: Set(other.id),
        workspace_id: Set(workspace_id),
        role: Set(entity::Role::Admin),
        ..Default::default()
    }
    .insert(&db)
    .await;
    assert!(dup.is_ok(), "first membership insert should succeed");
    assert!(dup2.is_err(), "duplicate membership must be rejected");
    // And granting the second user read access now opens the workspace.
    let res = app
        .get(&format!("/api/workspaces/{workspace_id}"))
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
}

/// Every error answers `{"error": "..."}` in sentence case, extractors' rejections included.
#[tokio::test]
async fn every_error_has_the_one_shape() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let body = |res: TestResponse| async move {
        let status = res.status_code();
        let json: serde_json::Value = serde_json::from_str(&res.text()).expect("a JSON error body");
        (
            status,
            json["error"]
                .as_str()
                .expect("an error message")
                .to_string(),
        )
    };
    // A path that does not parse.
    let (status, error) = body(
        app.get("/api/workspaces/not-a-uuid")
            .session(Some(&admin))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.starts_with(char::is_uppercase), "{error}");
    // Not found.
    let (status, error) = body(
        app.get(&format!("/api/workspaces/{}", uuid::Uuid::new_v4()))
            .session(Some(&admin))
            .await,
    )
    .await;
    assert_eq!(
        (status, error.as_str()),
        (StatusCode::NOT_FOUND, "Not found")
    );
    // A body that is not JSON, and JSON that does not fit.
    let req = app
        .post("/api/workspaces")
        .text("name=x")
        .session(Some(&admin));
    let (status, error) = body(req.await).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(error.contains("JSON"), "{error}");
    let (status, error) = body(
        app.post("/api/workspaces")
            .json_text(r#"{"name":1}"#)
            .session(Some(&admin))
            .await,
    )
    .await;
    assert_eq!(
        (status, error.as_str()),
        (StatusCode::BAD_REQUEST, "Invalid request body")
    );
    // A validation message.
    let (status, error) = body(
        app.post("/api/auth/bootstrap")
            .json_text(r#"{"email":"x","password":"long enough"}"#)
            .await,
    )
    .await;
    assert!(status.is_client_error());
    assert!(error.starts_with(char::is_uppercase), "{error}");
}
