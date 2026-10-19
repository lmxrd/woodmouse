//! Comments: markdown bodies, ownership rules, soft delete and restore.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::{sessions, users, workspace_members};
use serde_json::Value;

use crate::common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
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
        password_hash: Set("not-a-login-hash".to_string()),
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
async fn comments_lifecycle() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let writer = member(&db, ws, "w@home.io", entity::Role::Write).await;
    let reader = member(&db, ws, "r@home.io", entity::Role::Read).await;

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
    let task = json(
        request(
            &app,
            post_json(
                &format!("/api/lists/{list}/tasks"),
                r#"{"title":"T"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Markdown is stored as given (trimmed); empty comments are refused.
    let res = request(
        &app,
        post_json(
            &format!("/api/tasks/{task}/comments"),
            r#"{"body":"  **bold** and `code`\n- item  "}"#,
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let c = json(res).await;
    assert_eq!(c["body"], "**bold** and `code`\n- item");
    assert_eq!(c["author"]["email"], "w@home.io");
    assert!(c["edited_at"].is_null());
    let comment = c["id"].as_str().unwrap().to_string();
    let res = request(
        &app,
        post_json(
            &format!("/api/tasks/{task}/comments"),
            r#"{"body":"   "}"#,
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let t = json(request(&app, get(&format!("/api/tasks/{task}"), Some(&admin))).await).await;
    assert_eq!(t["comment_count"], 1);
    assert_eq!(t["attachment_count"], 0);

    // Readers can read but not write.
    let res = request(
        &app,
        post_json(
            &format!("/api/tasks/{task}/comments"),
            r#"{"body":"hi"}"#,
            Some(&reader),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        get(&format!("/api/tasks/{task}/comments"), Some(&reader)),
    )
    .await;
    assert_eq!(json(res).await.as_array().unwrap().len(), 1);

    // Only the author edits; the edit is marked.
    let res = request(
        &app,
        patch_json(
            &format!("/api/comments/{comment}"),
            r#"{"body":"changed"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "even admins do not edit other people's words"
    );
    let res = request(
        &app,
        patch_json(
            &format!("/api/comments/{comment}"),
            r#"{"body":"changed"}"#,
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let edited = json(res).await;
    assert_eq!(edited["body"], "changed");
    assert!(edited["edited_at"].is_string());

    // Its versions are kept, newest first, each with its text.
    let versions = json(
        request(
            &app,
            get(&format!("/api/comments/{comment}/revisions"), Some(&writer)),
        )
        .await,
    )
    .await;
    assert_eq!(versions.as_array().unwrap().len(), 2);
    assert_eq!(versions[0]["body"], "changed");
    assert_eq!(versions[1]["redacted"], false);

    // The author deletes; others without admin cannot delete someone else's.
    let other = json(
        request(
            &app,
            post_json(
                &format!("/api/tasks/{task}/comments"),
                r#"{"body":"from admin"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let res = request(
        &app,
        delete(&format!("/api/comments/{other}"), Some(&writer)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &app,
        delete(&format!("/api/comments/{comment}"), Some(&writer)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &app,
        get(&format!("/api/tasks/{task}/comments"), Some(&admin)),
    )
    .await;
    assert_eq!(json(res).await.as_array().unwrap().len(), 1);

    // An admin can delete anyone's; deleted comments are restorable from Trash.
    let res = request(
        &app,
        delete(&format!("/api/comments/{other}"), Some(&admin)),
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
    let items = trash.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|i| i["kind"] == "comment" && i["context"] == "on T")
    );
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/trash/comment/{comment}/restore"),
            "{}",
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &app,
        get(&format!("/api/tasks/{task}/comments"), Some(&admin)),
    )
    .await;
    assert_eq!(json(res).await[0]["body"], "changed");

    // A comment cannot be restored while its task is deleted; deleting a task hides its comments.
    request(
        &app,
        delete(&format!("/api/comments/{comment}"), Some(&admin)),
    )
    .await;
    request(&app, delete(&format!("/api/tasks/{task}"), Some(&admin))).await;
    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/trash/comment/{comment}/restore"),
            "{}",
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = request(
        &app,
        get(&format!("/api/tasks/{task}/comments"), Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
