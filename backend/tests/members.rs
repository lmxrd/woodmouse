//! Workspace members, invite links, and label counts.

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

async fn add_member(
    app: &axum::Router,
    cookie: &str,
    workspace_id: uuid::Uuid,
    email: &str,
    role: &str,
) -> axum::http::Response<axum::body::Body> {
    request(
        app,
        post_json(
            &format!("/api/workspaces/{workspace_id}/members"),
            &format!(r#"{{"name":"New Person","email":"{email}","role":"{role}"}}"#),
            Some(cookie),
        ),
    )
    .await
}

#[tokio::test]
async fn invite_link_flow() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let res = add_member(&app, &admin, ws, "Anders@Home.io", "write").await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json(res).await;
    assert_eq!(body["member"]["pending"], true);
    assert_eq!(body["member"]["user"]["email"], "anders@home.io");
    let token = body["invite_token"].as_str().expect("token").to_string();

    // The link is public and shows who it is for.
    let res = request(&app, get(&format!("/api/invites/{token}"), None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["email"], "anders@home.io");

    // No password yet, so no login — not even with an empty one.
    for password in ["", "whatever123"] {
        let res = request(
            &app,
            post_json(
                "/api/auth/login",
                &format!(r#"{{"email":"anders@home.io","password":"{password}"}}"#),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // Too-short password is rejected and does not consume the invite.
    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{token}/accept"),
            r#"{"password":"short"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{token}/accept"),
            r#"{"password":"a good password"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = cookie_of(&res);
    let res = request(&app, get("/api/auth/me", Some(&cookie))).await;
    assert_eq!(json(res).await["email"], "anders@home.io");

    // One-time: the link is dead now, and the new member is no longer pending.
    let res = request(&app, get(&format!("/api/invites/{token}"), None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{token}/accept"),
            r#"{"password":"another password"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        get(&format!("/api/workspaces/{ws}/members"), Some(&admin)),
    )
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
    let res = request(&app, get("/api/invites/nope", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
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
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/members/{user_id}/invite"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let fresh = json(res).await["invite_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(old, fresh);
    let res = request(&app, get(&format!("/api/invites/{old}"), None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(&app, get(&format!("/api/invites/{fresh}"), None)).await;
    assert_eq!(res.status(), StatusCode::OK);

    // Re-adding a pending member by email also issues a fresh link — but it
    // is a conflict because they are already in this workspace.
    let res = add_member(&app, &admin, ws, "sara@home.io", "read").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // An expired invite is dead.
    let expired = toasty::create!(woodmouse::models::UserInvite {
        token: "expired-token".to_string(),
        user_id: user_id.parse::<uuid::Uuid>().unwrap(),
        expires_at: "2000-01-01T00:00:00"
            .parse::<jiff::civil::DateTime>()
            .unwrap(),
    })
    .exec(&mut db.clone())
    .await
    .expect("seed invite");
    let res = request(&app, get(&format!("/api/invites/{}", expired.token), None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Once accepted, there is nothing left to reissue.
    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{fresh}/accept"),
            r#"{"password":"a good password"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/members/{user_id}/invite"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
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
    let res = request(
        &app,
        post_json(
            &format!("/api/invites/{token}/accept"),
            r#"{"password":"a good password"}"#,
            None,
        ),
    )
    .await;
    let sara = cookie_of(&res);

    // Second workspace: the existing account is simply added, no invite.
    let res = add_member(&app, &admin, work, "SARA@home.io", "read").await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json(res).await;
    assert_eq!(body["invite_token"], Value::Null);
    assert_eq!(body["member"]["pending"], false);
    assert_eq!(body["member"]["role"], "read");

    // A write-role member can list but not add, change or remove.
    let res = request(
        &app,
        get(&format!("/api/workspaces/{home}/members"), Some(&sara)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = add_member(&app, &sara, home, "x@home.io", "read").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let sara_id = body["member"]["user"]["id"].as_str().unwrap();
    let res = request(
        &app,
        patch_json(
            &format!("/api/workspaces/{home}/members/{sara_id}"),
            r#"{"role":"admin"}"#,
            Some(&sara),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Bad input.
    let res = add_member(&app, &admin, home, "not-an-email", "read").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn roles_removal_and_last_admin_guard() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let me = json(request(&app, get("/api/auth/me", Some(&admin))).await).await;
    let admin_id = me["id"].as_str().unwrap().to_string();

    // The only admin cannot be demoted or removed.
    let res = request(
        &app,
        patch_json(
            &format!("/api/workspaces/{ws}/members/{admin_id}"),
            r#"{"role":"write"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = request(
        &app,
        delete(
            &format!("/api/workspaces/{ws}/members/{admin_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // A project, list and task with the new member assigned.
    let project = json(
        request(
            &app,
            post_json(
                &format!("/api/workspaces/{ws}/projects"),
                r#"{"name":"P"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = json(
        request(
            &app,
            post_json(
                &format!("/api/projects/{project}/lists"),
                r#"{"name":"L"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let body = json(add_member(&app, &admin, ws, "sara@home.io", "write").await).await;
    let sara_id = body["member"]["user"]["id"].as_str().unwrap().to_string();
    let res = request(
        &app,
        post_json(
            &format!("/api/lists/{list}/tasks"),
            &format!(r#"{{"title":"T","assignee_ids":["{sara_id}"]}}"#),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let task_id = json(res).await["id"].as_str().unwrap().to_string();

    // Promote, then the original admin may step down.
    let res = request(
        &app,
        patch_json(
            &format!("/api/workspaces/{ws}/members/{sara_id}"),
            r#"{"role":"admin"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["role"], "admin");
    let res = request(
        &app,
        patch_json(
            &format!("/api/workspaces/{ws}/members/{admin_id}"),
            r#"{"role":"read"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // Removing a member also unassigns them. (The bootstrap user is a super
    // admin, so it keeps admin access regardless of its membership role.)
    let res = request(
        &app,
        delete(
            &format!("/api/workspaces/{ws}/members/{sara_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::CONFLICT,
        "sara is now the only admin"
    );
    let res = request(
        &app,
        patch_json(
            &format!("/api/workspaces/{ws}/members/{admin_id}"),
            r#"{"role":"admin"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = request(
        &app,
        delete(
            &format!("/api/workspaces/{ws}/members/{sara_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let task = json(request(&app, get(&format!("/api/tasks/{task_id}"), Some(&admin))).await).await;
    assert_eq!(task["assignees"].as_array().unwrap().len(), 0);

    let res = request(
        &app,
        delete(
            &format!("/api/workspaces/{ws}/members/{sara_id}"),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "already removed");
}

#[tokio::test]
async fn label_counts() {
    let app = app().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let label = json(
        request(
            &app,
            post_json(
                &format!("/api/workspaces/{ws}/labels"),
                r#"{"name":"Materials"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/labels"),
            r#"{"name":"Unused"}"#,
            Some(&admin),
        ),
    )
    .await;

    let project = json(
        request(
            &app,
            post_json(
                &format!("/api/workspaces/{ws}/projects"),
                r#"{"name":"P"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = json(
        request(
            &app,
            post_json(
                &format!("/api/projects/{project}/lists"),
                r#"{"name":"L"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    for title in ["A", "B"] {
        request(
            &app,
            post_json(
                &format!("/api/lists/{list}/tasks"),
                &format!(r#"{{"title":"{title}","label_ids":["{label}"]}}"#),
                Some(&admin),
            ),
        )
        .await;
    }

    let labels = json(
        request(
            &app,
            get(&format!("/api/workspaces/{ws}/labels"), Some(&admin)),
        )
        .await,
    )
    .await;
    let labels = labels.as_array().unwrap();
    assert_eq!(labels[0]["name"], "Materials");
    assert_eq!(labels[0]["task_count"], 2);
    assert_eq!(labels[1]["name"], "Unused");
    assert_eq!(labels[1]["task_count"], 0);
}
