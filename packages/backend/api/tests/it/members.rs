//! Workspace members, invite links, and label counts.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::user_invites;
use serde_json::Value;

use crate::common::*;

async fn json(res: TestResponse) -> Value {
    serde_json::from_str(&res.text()).expect("valid json")
}

async fn add_member(
    app: &TestServer,
    cookie: &str,
    workspace_id: uuid::Uuid,
    email: &str,
    role: &str,
) -> TestResponse {
    app.post(&format!("/api/workspaces/{workspace_id}/members"))
        .json_text(&format!(
            r#"{{"name":"New Person","email":"{email}","role":"{role}"}}"#
        ))
        .session(Some(cookie))
        .await
}

#[tokio::test]
async fn invite_link_flow() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let res = add_member(&app, &admin, ws, "Anders@Home.io", "write").await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let body = json(res).await;
    assert_eq!(body["member"]["pending"], true);
    assert_eq!(body["member"]["user"]["email"], "anders@home.io");
    let token = body["invite_token"].as_str().expect("token").to_string();

    // The link is public and shows who it is for.
    let res = app.get(&format!("/api/invites/{token}")).await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(json(res).await["email"], "anders@home.io");

    // No password yet, so no login — not even with an empty one.
    for password in ["", "whatever123"] {
        let res = app
            .post("/api/auth/login")
            .json_text(&format!(
                r#"{{"email":"anders@home.io","password":"{password}"}}"#
            ))
            .await;
        assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
    }

    // Too-short password is rejected and does not consume the invite.
    let res = app
        .post(&format!("/api/invites/{token}/accept"))
        .json_text(r#"{"password":"short"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    let res = app
        .post(&format!("/api/invites/{token}/accept"))
        .json_text(r#"{"password":"a good password"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let cookie = cookie_of(&res);
    let res = app.get("/api/auth/me").session(Some(&cookie)).await;
    assert_eq!(json(res).await["email"], "anders@home.io");

    // One-time: the link is dead now, and the new member is no longer pending.
    let res = app.get(&format!("/api/invites/{token}")).await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app
        .post(&format!("/api/invites/{token}/accept"))
        .json_text(r#"{"password":"another password"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app
        .get(&format!("/api/workspaces/{ws}/members"))
        .session(Some(&admin))
        .await;
    let members = json(res).await;
    let anders = members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["user"]["email"] == "anders@home.io")
        .expect("member listed");
    assert_eq!(anders["pending"], false);

    // Unknown tokens are a plain 404.
    let res = app.get("/api/invites/nope").await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn expired_and_reissued_invites() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let body = json(add_member(&app, &admin, ws, "sara@home.io", "read").await).await;
    let old = body["invite_token"].as_str().unwrap().to_string();
    let user_id = body["member"]["user"]["id"].as_str().unwrap().to_string();

    // Reissuing replaces the old link.
    let res = app
        .post(&format!("/api/workspaces/{ws}/members/{user_id}/invite"))
        .json_text("{}")
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let fresh = json(res).await["invite_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(old, fresh);
    let res = app.get(&format!("/api/invites/{old}")).await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app.get(&format!("/api/invites/{fresh}")).await;
    assert_eq!(res.status_code(), StatusCode::OK);

    // Re-adding a pending member by email also issues a fresh link — but it
    // is a conflict because they are already in this workspace.
    let res = add_member(&app, &admin, ws, "sara@home.io", "read").await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);

    // An expired invite is dead.
    let expired = user_invites::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        token: Set("expired-token".to_string()),
        user_id: Set(user_id.parse::<uuid::Uuid>().unwrap()),
        expires_at: Set(api::time::DateTime(
            "2000-01-01T00:00:00"
                .parse::<jiff::civil::DateTime>()
                .unwrap(),
        )),
    }
    .insert(&db)
    .await
    .expect("seed invite");
    let res = app.get(&format!("/api/invites/{}", expired.token)).await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // Once accepted, there is nothing left to reissue.
    let res = app
        .post(&format!("/api/invites/{fresh}/accept"))
        .json_text(r#"{"password":"a good password"}"#)
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let res = app
        .post(&format!("/api/workspaces/{ws}/members/{user_id}/invite"))
        .json_text("{}")
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn existing_account_is_added_directly_and_only_admins_add() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let home = create_workspace(&app, &admin, "Home").await;
    let work = create_workspace(&app, &admin, "Work").await;

    // First workspace: new account + accepted invite.
    let body = json(add_member(&app, &admin, home, "sara@home.io", "write").await).await;
    let token = body["invite_token"].as_str().unwrap().to_string();
    let res = app
        .post(&format!("/api/invites/{token}/accept"))
        .json_text(r#"{"password":"a good password"}"#)
        .await;
    let sara = cookie_of(&res);

    // Second workspace: the existing account is simply added, no invite.
    let res = add_member(&app, &admin, work, "SARA@home.io", "read").await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let body = json(res).await;
    assert_eq!(body["invite_token"], Value::Null);
    assert_eq!(body["member"]["pending"], false);
    assert_eq!(body["member"]["role"], "read");

    // A write-role member can list but not add, change or remove.
    let res = app
        .get(&format!("/api/workspaces/{home}/members"))
        .session(Some(&sara))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let res = add_member(&app, &sara, home, "x@home.io", "read").await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let sara_id = body["member"]["user"]["id"].as_str().unwrap();
    let res = app
        .patch(&format!("/api/workspaces/{home}/members/{sara_id}"))
        .json_text(r#"{"role":"admin"}"#)
        .session(Some(&sara))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // Bad input.
    let res = add_member(&app, &admin, home, "not-an-email", "read").await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn roles_removal_and_last_admin_guard() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let me = json(app.get("/api/auth/me").session(Some(&admin)).await).await;
    let admin_id = me["id"].as_str().unwrap().to_string();

    // The only admin cannot be demoted or removed.
    let res = app
        .patch(&format!("/api/workspaces/{ws}/members/{admin_id}"))
        .json_text(r#"{"role":"write"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);
    let res = app
        .delete(&format!("/api/workspaces/{ws}/members/{admin_id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);

    // A project, list and task with the new member assigned.
    let project = json(
        app.post(&format!("/api/workspaces/{ws}/projects"))
            .json_text(r#"{"name":"P"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = json(
        app.post(&format!("/api/projects/{project}/lists"))
            .json_text(r#"{"name":"L"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let body = json(add_member(&app, &admin, ws, "sara@home.io", "write").await).await;
    let sara_id = body["member"]["user"]["id"].as_str().unwrap().to_string();
    let res = app
        .post(&format!("/api/lists/{list}/tasks"))
        .json_text(&format!(r#"{{"title":"T","assignee_ids":["{sara_id}"]}}"#))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let task_id = json(res).await["id"].as_str().unwrap().to_string();

    // Promote, then the original admin may step down.
    let res = app
        .patch(&format!("/api/workspaces/{ws}/members/{sara_id}"))
        .json_text(r#"{"role":"admin"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(json(res).await["role"], "admin");
    let res = app
        .patch(&format!("/api/workspaces/{ws}/members/{admin_id}"))
        .json_text(r#"{"role":"read"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);

    // Removing a member also unassigns them. (The bootstrap user is a super
    // admin, so it keeps admin access regardless of its membership role.)
    let res = app
        .delete(&format!("/api/workspaces/{ws}/members/{sara_id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(
        res.status_code(),
        StatusCode::CONFLICT,
        "sara is now the only admin"
    );
    let res = app
        .patch(&format!("/api/workspaces/{ws}/members/{admin_id}"))
        .json_text(r#"{"role":"admin"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let res = app
        .delete(&format!("/api/workspaces/{ws}/members/{sara_id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let task = json(
        app.get(&format!("/api/tasks/{task_id}"))
            .session(Some(&admin))
            .await,
    )
    .await;
    assert_eq!(task["assignees"].as_array().unwrap().len(), 0);

    let res = app
        .delete(&format!("/api/workspaces/{ws}/members/{sara_id}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND, "already removed");
}

#[tokio::test]
async fn label_counts() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let label = json(
        app.post(&format!("/api/workspaces/{ws}/labels"))
            .json_text(r#"{"name":"Materials"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.post(&format!("/api/workspaces/{ws}/labels"))
        .json_text(r#"{"name":"Unused"}"#)
        .session(Some(&admin))
        .await;

    let project = json(
        app.post(&format!("/api/workspaces/{ws}/projects"))
            .json_text(r#"{"name":"P"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = json(
        app.post(&format!("/api/projects/{project}/lists"))
            .json_text(r#"{"name":"L"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    for title in ["A", "B"] {
        app.post(&format!("/api/lists/{list}/tasks"))
            .json_text(&format!(r#"{{"title":"{title}","label_ids":["{label}"]}}"#))
            .session(Some(&admin))
            .await;
    }

    let labels = json(
        app.get(&format!("/api/workspaces/{ws}/labels"))
            .session(Some(&admin))
            .await,
    )
    .await;
    let labels = labels.as_array().unwrap();
    assert_eq!(labels[0]["name"], "Materials");
    assert_eq!(labels[0]["task_count"], 2);
    assert_eq!(labels[1]["name"], "Unused");
    assert_eq!(labels[1]["task_count"], 0);
}
