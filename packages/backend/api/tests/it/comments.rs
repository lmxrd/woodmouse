//! Comments: markdown bodies, ownership rules, soft delete and restore.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::{users, workspace_members};
use serde_json::Value;

use crate::common::*;

async fn json(res: TestResponse) -> Value {
    serde_json::from_str(&res.text()).expect("valid json")
}

async fn member(
    db: &sea_orm::DatabaseConnection,
    ws: uuid::Uuid,
    email: &str,
    role: entity::Role,
) -> String {
    let user = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set("".to_string()),
        password_hash: Set(Some("not-a-login-hash".to_string())),
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
    let token = api::sessions::start_session(db, user.id).await.unwrap();
    format!("woodmouse_session={token}")
}

#[tokio::test]
async fn comments_lifecycle() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let writer = member(&db, ws, "w@home.io", entity::Role::Write).await;
    let reader = member(&db, ws, "r@home.io", entity::Role::Read).await;

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
    let task = json(
        app.post(&format!("/api/lists/{list}/tasks"))
            .json_text(r#"{"title":"T"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Markdown is stored as given (trimmed); empty comments are refused.
    let res = app
        .post(&format!("/api/tasks/{task}/comments"))
        .json_text(r#"{"body":"  **bold** and `code`\n- item  "}"#)
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let c = json(res).await;
    assert_eq!(c["body"], "**bold** and `code`\n- item");
    assert_eq!(c["author"]["email"], "w@home.io");
    assert!(c["edited_at"].is_null());
    let comment = c["id"].as_str().unwrap().to_string();
    let res = app
        .post(&format!("/api/tasks/{task}/comments"))
        .json_text(r#"{"body":"   "}"#)
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    let t = json(
        app.get(&format!("/api/tasks/{task}"))
            .session(Some(&admin))
            .await,
    )
    .await;
    assert_eq!(t["comment_count"], 1);
    assert_eq!(t["attachment_count"], 0);

    // Readers can read but not write.
    let res = app
        .post(&format!("/api/tasks/{task}/comments"))
        .json_text(r#"{"body":"hi"}"#)
        .session(Some(&reader))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app
        .get(&format!("/api/tasks/{task}/comments"))
        .session(Some(&reader))
        .await;
    assert_eq!(json(res).await.as_array().unwrap().len(), 1);

    // Only the author edits; the edit is marked.
    let res = app
        .patch(&format!("/api/comments/{comment}"))
        .json_text(r#"{"body":"changed"}"#)
        .session(Some(&admin))
        .await;
    assert_eq!(
        res.status_code(),
        StatusCode::NOT_FOUND,
        "even admins do not edit other people's words"
    );
    let res = app
        .patch(&format!("/api/comments/{comment}"))
        .json_text(r#"{"body":"changed"}"#)
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let edited = json(res).await;
    assert_eq!(edited["body"], "changed");
    assert!(edited["edited_at"].is_string());

    // Its versions are kept, newest first, each with its text.
    let versions = json(
        app.get(&format!("/api/comments/{comment}/revisions"))
            .session(Some(&writer))
            .await,
    )
    .await;
    assert_eq!(versions.as_array().unwrap().len(), 2);
    assert_eq!(versions[0]["body"], "changed");
    assert_eq!(versions[1]["redacted"], false);

    // The author deletes; others without admin cannot delete someone else's.
    let other = json(
        app.post(&format!("/api/tasks/{task}/comments"))
            .json_text(r#"{"body":"from admin"}"#)
            .session(Some(&admin))
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let res = app
        .delete(&format!("/api/comments/{other}"))
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    let res = app
        .delete(&format!("/api/comments/{comment}"))
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = app
        .get(&format!("/api/tasks/{task}/comments"))
        .session(Some(&admin))
        .await;
    assert_eq!(json(res).await.as_array().unwrap().len(), 1);

    // An admin can delete anyone's; deleted comments are restorable from Trash.
    let res = app
        .delete(&format!("/api/comments/{other}"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let trash = json(
        app.get(&format!("/api/workspaces/{ws}/trash"))
            .session(Some(&admin))
            .await,
    )
    .await;
    let items = trash.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|i| i["kind"] == "comment" && i["context"] == "on T")
    );
    let res = app
        .post(&format!(
            "/api/workspaces/{ws}/trash/comment/{comment}/restore"
        ))
        .json_text("{}")
        .session(Some(&writer))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = app
        .get(&format!("/api/tasks/{task}/comments"))
        .session(Some(&admin))
        .await;
    assert_eq!(json(res).await[0]["body"], "changed");

    // A comment cannot be restored while its task is deleted; deleting a task hides its comments.
    app.delete(&format!("/api/comments/{comment}"))
        .session(Some(&admin))
        .await;
    app.delete(&format!("/api/tasks/{task}"))
        .session(Some(&admin))
        .await;
    let res = app
        .post(&format!(
            "/api/workspaces/{ws}/trash/comment/{comment}/restore"
        ))
        .json_text("{}")
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::CONFLICT);
    let res = app
        .get(&format!("/api/tasks/{task}/comments"))
        .session(Some(&admin))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}
