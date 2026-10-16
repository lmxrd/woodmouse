//! The site admin: users, deleted workspaces, the audit log, the integrity check and backups.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::*;

async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
}

/// A second, ordinary user with a session; returns (id, cookie).
async fn seed_user(db: &toasty::Db, email: &str) -> (uuid::Uuid, String) {
    let user = toasty::create!(woodmouse::models::User {
        email: email.to_string(),
        name: "".to_string(),
        password_hash: "x".to_string(),
        super_admin: false,
        active: true,
    })
    .exec(&mut db.clone())
    .await
    .unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    toasty::create!(woodmouse::models::Session {
        token: token.clone(),
        user_id: user.id,
        expires_at: far_future(),
    })
    .exec(&mut db.clone())
    .await
    .unwrap();
    (user.id, format!("woodmouse_session={token}"))
}

#[tokio::test]
async fn only_the_super_admin_gets_in() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let (_, other) = seed_user(&db, "other@home.io").await;
    for uri in [
        "/api/admin/users",
        "/api/admin/audit",
        "/api/admin/integrity",
    ] {
        let res = request(&app, get(uri, Some(&other))).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        let res = request(&app, get(uri, Some(&admin))).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn disabling_a_user_ends_their_access_and_enabling_restores_it() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let (id, cookie) = seed_user(&db, "sara@home.io").await;
    assert_eq!(
        request(&app, get("/api/auth/me", Some(&cookie)))
            .await
            .status(),
        StatusCode::OK
    );

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
    assert_eq!(json_of(res).await["active"], false);
    assert_eq!(
        request(&app, get("/api/auth/me", Some(&cookie)))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "their sessions are gone"
    );

    // A global role is set and cleared with null.
    let res = request(
        &app,
        patch_json(
            &format!("/api/admin/users/{id}"),
            r#"{"active":true,"global_role":"write"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(json_of(res).await["global_role"], "write");
    let res = request(
        &app,
        patch_json(
            &format!("/api/admin/users/{id}"),
            r#"{"global_role":null}"#,
            Some(&admin),
        ),
    )
    .await;
    assert!(json_of(res).await["global_role"].is_null());

    // The log records what happened.
    let res = request(&app, get("/api/admin/audit?limit=50", Some(&admin))).await;
    let log = json_of(res).await;
    let actions: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for wanted in ["user_disabled", "user_enabled", "role_changed"] {
        assert!(actions.contains(&wanted), "{wanted} in {actions:?}");
    }
}

#[tokio::test]
async fn deleted_workspaces_are_restored_by_the_super_admin() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let (_, other) = seed_user(&db, "other@home.io").await;

    let res = request(&app, delete(&format!("/api/workspaces/{ws}"), Some(&admin))).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        request(&app, get(&format!("/api/workspaces/{ws}"), Some(&admin)))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let deleted =
        json_of(request(&app, get("/api/admin/workspaces/deleted", Some(&admin))).await).await;
    assert_eq!(deleted.as_array().unwrap().len(), 1);
    assert_eq!(deleted[0]["name"], "Home");

    let res = request(
        &app,
        post_json(
            &format!("/api/admin/workspaces/{ws}/restore"),
            "{}",
            Some(&other),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        post_json(
            &format!("/api/admin/workspaces/{ws}/restore"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        request(&app, get(&format!("/api/workspaces/{ws}"), Some(&admin)))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_integrity_check_finds_orphans_and_redaction_removes_content() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    // A normal database is clean.
    let res = request(&app, get("/api/admin/integrity", Some(&admin))).await;
    assert_eq!(json_of(res).await, json!([]));

    // A membership of a workspace that does not exist is reported.
    let user = seed_user(&db, "x@home.io").await.0;
    toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: user,
        workspace_id: uuid::Uuid::new_v4(),
        role: woodmouse::models::Role::Read
    })
    .exec(&mut db.clone())
    .await
    .unwrap();
    let problems = json_of(request(&app, get("/api/admin/integrity", Some(&admin))).await).await;
    assert_eq!(problems.as_array().unwrap().len(), 1);
    assert_eq!(problems[0]["table"], "workspace_members");

    // Redacting a page version removes its content but keeps the version.
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/pages"),
            r#"{"title":"Secret"}"#,
            Some(&admin),
        ),
    )
    .await;
    let page = json_of(res).await["id"].as_str().unwrap().to_string();
    request(
        &app,
        put_json(
            &format!("/api/pages/{page}/blocks"),
            r#"{"blocks":[{"type":"paragraph","spans":[{"text":"password is hunter2"}]}]}"#,
            Some(&admin),
        ),
    )
    .await;
    let list = json_of(
        request(
            &app,
            get(&format!("/api/pages/{page}/revisions"), Some(&admin)),
        )
        .await,
    )
    .await;
    let rev = list[0]["id"].as_str().unwrap().to_string();
    let res = request(
        &app,
        post_json(
            &format!("/api/admin/revisions/{rev}/redact"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let detail = json_of(
        request(
            &app,
            get(&format!("/api/page-revisions/{rev}"), Some(&admin)),
        )
        .await,
    )
    .await;
    assert_eq!(detail["redacted"], true);
    assert_eq!(detail["blocks"], json!([]));
}
