//! The site admin: users, deleted workspaces, the audit log, and the integrity check.

use axum::http::StatusCode;
use serde_json::{Value, json};
use woodmouse::db::prelude::*;
use woodmouse::models::{sessions, users};

use crate::common::*;

async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
}

/// A second, ordinary user with a session; returns (id, cookie).
async fn seed_user(db: &sea_orm::DatabaseConnection, email: &str) -> (uuid::Uuid, String) {
    let user = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set("".to_string()),
        password_hash: Set("x".to_string()),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    sessions::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        token: Set(token.clone()),
        user_id: Set(user.id),
        expires_at: Set(far_future()),
    }
    .insert(db)
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
    let all = json_of(request(&app, get("/api/admin/workspaces", Some(&admin))).await).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(all[0]["name"], "Home");
    assert!(all[0]["deleted_at"].is_string());
    assert_eq!(all[0]["members"], 1);

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

    // A membership of a workspace that does not exist is reported. The database refuses to make one, so
    // the row is written with foreign keys off, as a manual edit or a partial restore could.
    let user = seed_user(&db, "x@home.io").await.0;
    let mut conn = db.get_sqlite_connection_pool().acquire().await.unwrap();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workspace_members (user_id, workspace_id, role) VALUES (?1, ?2, 'read')",
    )
    .bind(user)
    .bind(uuid::Uuid::new_v4())
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
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

#[tokio::test]
async fn the_super_admin_creates_edits_and_reinvites_users() {
    let (app, _) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let res = request(
        &app,
        post_json(
            "/api/admin/users",
            &format!(
                r#"{{"name":"Sara","email":"Sara@Home.io","global_role":"read","memberships":[{{"workspace_id":"{ws}","role":"write"}}]}}"#
            ),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = json_of(res).await;
    let id = created["user"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["user"]["email"], "sara@home.io");
    assert_eq!(created["user"]["pending"], true);
    assert_eq!(created["user"]["global_role"], "read");
    assert_eq!(created["user"]["memberships"][0]["workspace_name"], "Home");
    assert_eq!(created["user"]["memberships"][0]["role"], "write");
    let token = created["invite_token"].as_str().unwrap().to_string();

    // The same email twice is a conflict.
    let res = request(
        &app,
        post_json(
            "/api/admin/users",
            r#"{"email":"sara@home.io"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // A fresh invite replaces the old one.
    let res = request(
        &app,
        post_json(&format!("/api/admin/users/{id}/invite"), "{}", Some(&admin)),
    )
    .await;
    let fresh = json_of(res).await["invite_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(fresh, token);
    assert_eq!(
        request(&app, get(&format!("/api/invites/{token}"), None))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // Rename and change the email.
    let res = request(
        &app,
        patch_json(
            &format!("/api/admin/users/{id}"),
            r#"{"name":"Sara W","email":"sara.w@home.io"}"#,
            Some(&admin),
        ),
    )
    .await;
    let updated = json_of(res).await;
    assert_eq!(updated["name"], "Sara W");
    assert_eq!(updated["email"], "sara.w@home.io");

    // Once they accept, they have a password and no more invites.
    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{fresh}/accept"),
            r#"{"password":"a good password"}"#,
            None,
        ),
    )
    .await;
    assert!(res.status().is_success());
    let res = request(
        &app,
        post_json(&format!("/api/admin/users/{id}/invite"), "{}", Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}
