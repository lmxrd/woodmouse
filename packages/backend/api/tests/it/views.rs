//! Saved views: private vs shared, who may edit, and trash.

use axum::http::StatusCode;
use serde_json::Value;
use woodmouse::db::prelude::*;
use woodmouse::models::{sessions, users, workspace_members};

use crate::common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

async fn member(
    db: &sea_orm::DatabaseConnection,
    ws: uuid::Uuid,
    email: &str,
    role: woodmouse::models::Role,
) -> String {
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
    workspace_members::ActiveModel {
        user_id: Set(user.id),
        workspace_id: Set(ws),
        role: Set(role),
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
        expires_at: Set(crate::common::far_future()),
    }
    .insert(db)
    .await
    .unwrap();
    format!("woodmouse_session={token}")
}

#[tokio::test]
async fn private_and_shared_views() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let writer = member(&db, ws, "w@home.io", woodmouse::models::Role::Write).await;
    let reader = member(&db, ws, "r@home.io", woodmouse::models::Role::Read).await;
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

    let create = |cookie: String, body: &'static str| {
        let app = app.clone();
        let uri = format!("/api/projects/{project}/views");
        async move { request(&app, post_json(&uri, body, Some(&cookie))).await }
    };
    let list = |cookie: String| {
        let app = app.clone();
        let uri = format!("/api/projects/{project}/views");
        async move {
            json(request(&app, get(&uri, Some(&cookie))).await)
                .await
                .as_array()
                .unwrap()
                .clone()
        }
    };

    // Everyone may keep private views; only writers may share.
    let res = create(
        reader.clone(),
        r#"{"name":"Mine","params":{"view":"list","status":"open"}}"#,
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let mine = json(res).await;
    assert_eq!(mine["is_shared"], false);
    assert_eq!(mine["can_edit"], true);
    assert_eq!(mine["params"]["view"], "list");
    let res = create(
        reader.clone(),
        r#"{"name":"Nope","params":{},"is_shared":true}"#,
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = create(
        writer.clone(),
        r#"{"name":"Due this week","params":{"sort":"due_date"},"is_shared":true}"#,
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let shared = json(res).await;
    let shared_id = shared["id"].as_str().unwrap().to_string();
    create(writer.clone(), r#"{"name":"Writer private","params":{}}"#).await;

    // Visibility: shared for all, private only for its owner.
    let names = |views: &[Value]| -> Vec<String> {
        views
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        names(&list(reader.clone()).await),
        ["Due this week", "Mine"]
    );
    assert_eq!(
        names(&list(writer.clone()).await),
        ["Due this week", "Writer private"]
    );
    assert_eq!(
        names(&list(admin.clone()).await),
        ["Due this week"],
        "admins do not see other people's private views"
    );

    // Editing: owner always; an admin may change shared views; others may not.
    let patch = |cookie: &str, id: &str, body: &str| {
        let app = app.clone();
        let (cookie, uri, body) = (
            cookie.to_string(),
            format!("/api/views/{id}"),
            body.to_string(),
        );
        async move {
            request(&app, patch_json(&uri, &body, Some(&cookie)))
                .await
                .status()
        }
    };
    assert_eq!(
        patch(&reader, &shared_id, r#"{"name":"hijack"}"#).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        patch(&admin, &shared_id, r#"{"name":"Due soon"}"#).await,
        StatusCode::OK
    );
    assert_eq!(
        patch(&writer, &shared_id, r#"{"params":{"sort":"title"}}"#).await,
        StatusCode::OK
    );
    assert_eq!(
        patch(&writer, &shared_id, r#"{"name":""}"#).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        patch(&writer, &shared_id, r#"{"params":"not an object"}"#).await,
        StatusCode::BAD_REQUEST
    );
    let mine_id = mine["id"].as_str().unwrap();
    assert_eq!(
        patch(&admin, mine_id, r#"{"name":"x"}"#).await,
        StatusCode::NOT_FOUND,
        "private to its owner"
    );
    assert_eq!(
        patch(&reader, mine_id, r#"{"is_shared":true}"#).await,
        StatusCode::NOT_FOUND,
        "a reader cannot share"
    );

    // Delete moves it to Trash; the owner sees it there and can restore it.
    let res = request(
        &app,
        delete(&format!("/api/views/{mine_id}"), Some(&reader)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(names(&list(reader.clone()).await), ["Due soon"]);
    let res = request(
        &app,
        delete(&format!("/api/views/{shared_id}"), Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let trash = json(
        request(
            &app,
            get(&format!("/api/workspaces/{ws}/trash"), Some(&admin)),
        )
        .await,
    )
    .await;
    assert_eq!(
        trash.as_array().unwrap().len(),
        1,
        "the admin sees the shared view, not someone's private one"
    );
    assert_eq!(trash[0]["kind"], "view");
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/trash/view/{shared_id}/restore"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        names(&list(writer.clone()).await),
        ["Due soon", "Writer private"]
    );
}
