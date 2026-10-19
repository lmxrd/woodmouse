//! Soft delete and Trash: deleted things disappear everywhere, cascade as a group, and restore.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::{sessions, users, workspace_members};
use serde_json::Value;

use crate::common::*;

struct Fx {
    app: axum::Router,
    cookie: String,
    ws: uuid::Uuid,
    project: String,
    list: String,
}

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

async fn make(app: &axum::Router, cookie: &str, uri: &str, body: &str) -> String {
    let res = request(app, post_json(uri, body, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::CREATED, "POST {uri}");
    json(res).await["id"].as_str().unwrap().to_string()
}

async fn fixture() -> Fx {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &cookie, "Home").await;
    let project = make(
        &app,
        &cookie,
        &format!("/api/workspaces/{ws}/projects"),
        r#"{"name":"P"}"#,
    )
    .await;
    let list = make(
        &app,
        &cookie,
        &format!("/api/projects/{project}/lists"),
        r#"{"name":"L"}"#,
    )
    .await;
    Fx {
        app,
        cookie,
        ws,
        project,
        list,
    }
}

async fn trash(fx: &Fx) -> Vec<Value> {
    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{}/trash", fx.ws),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    json(res).await.as_array().unwrap().clone()
}

async fn task_titles(fx: &Fx) -> Vec<String> {
    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{}/tasks", fx.ws),
            Some(&fx.cookie),
        ),
    )
    .await;
    json(res)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_string())
        .collect()
}

async fn restore(fx: &Fx, kind: &str, id: &str) -> StatusCode {
    request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/trash/{kind}/{id}/restore", fx.ws),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await
    .status()
}

#[tokio::test]
async fn deleting_a_task_hides_it_and_restore_brings_it_back() {
    let fx = fixture().await;
    let a = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/lists/{}/tasks", fx.list),
        r#"{"title":"Alpha"}"#,
    )
    .await;
    make(
        &fx.app,
        &fx.cookie,
        &format!("/api/lists/{}/tasks", fx.list),
        r#"{"title":"Beta"}"#,
    )
    .await;

    let res = request(
        &fx.app,
        delete(&format!("/api/tasks/{a}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert_eq!(task_titles(&fx).await, vec!["Beta"]);
    let res = request(&fx.app, get(&format!("/api/tasks/{a}"), Some(&fx.cookie))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{a}"),
            r#"{"title":"x"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let items = trash(&fx).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "task");
    assert_eq!(items[0]["name"], "Alpha");
    assert_eq!(items[0]["context"], "P / L");
    assert_eq!(items[0]["deleted_by"]["email"], "admin@home.io");

    assert_eq!(restore(&fx, "task", &a).await, StatusCode::NO_CONTENT);
    assert_eq!(task_titles(&fx).await.len(), 2);
    assert!(trash(&fx).await.is_empty());
    // Not in the trash anymore.
    assert_eq!(restore(&fx, "task", &a).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn project_and_list_deletes_cascade_and_restore_as_a_group() {
    let fx = fixture().await;
    let keep = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/lists/{}/tasks", fx.list),
        r#"{"title":"Keep"}"#,
    )
    .await;
    let gone = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/lists/{}/tasks", fx.list),
        r#"{"title":"Gone early"}"#,
    )
    .await;

    // One task is deleted on its own first.
    request(
        &fx.app,
        delete(&format!("/api/tasks/{gone}"), Some(&fx.cookie)),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // Deleting the project takes the list and the remaining task with it.
    let res = request(
        &fx.app,
        delete(&format!("/api/projects/{}", fx.project), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(task_titles(&fx).await.is_empty());
    let res = request(
        &fx.app,
        get(
            &format!("/api/projects/{}/lists", fx.project),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Trash shows the project (with its task counted) and the early task, not the cascaded children.
    let items = trash(&fx).await;
    let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(items.len(), 2, "{kinds:?}");
    let project = items.iter().find(|i| i["kind"] == "project").unwrap();
    assert_eq!(project["child_count"], 1);

    // Restoring the early task alone is refused while its project is deleted.
    assert_eq!(restore(&fx, "task", &gone).await, StatusCode::CONFLICT);

    assert_eq!(
        restore(&fx, "project", &fx.project).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        task_titles(&fx).await,
        vec!["Keep"],
        "only what was deleted with the project returns"
    );
    let res = request(
        &fx.app,
        get(
            &format!("/api/projects/{}/lists", fx.project),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let _ = keep;

    // The early task is still in the trash and can now be restored.
    assert_eq!(trash(&fx).await.len(), 1);
    assert_eq!(restore(&fx, "task", &gone).await, StatusCode::NO_CONTENT);
    assert_eq!(task_titles(&fx).await.len(), 2);

    // List delete + restore.
    request(
        &fx.app,
        delete(&format!("/api/lists/{}", fx.list), Some(&fx.cookie)),
    )
    .await;
    assert!(task_titles(&fx).await.is_empty());
    assert_eq!(trash(&fx).await[0]["kind"], "list");
    assert_eq!(restore(&fx, "list", &fx.list).await, StatusCode::NO_CONTENT);
    assert_eq!(task_titles(&fx).await.len(), 2);
}

#[tokio::test]
async fn deleted_labels_disappear_from_tasks_and_counts_and_return_on_restore() {
    let fx = fixture().await;
    let label = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/workspaces/{}/labels", fx.ws),
        r#"{"name":"Materials"}"#,
    )
    .await;
    let task = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/lists/{}/tasks", fx.list),
        &format!(r#"{{"title":"T","label_ids":["{label}"]}}"#),
    )
    .await;

    request(
        &fx.app,
        delete(&format!("/api/labels/{label}"), Some(&fx.cookie)),
    )
    .await;
    let t = json(
        request(
            &fx.app,
            get(&format!("/api/tasks/{task}"), Some(&fx.cookie)),
        )
        .await,
    )
    .await;
    assert_eq!(t["labels"].as_array().unwrap().len(), 0);
    let labels = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/labels", fx.ws),
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(labels.as_array().unwrap().len(), 0);
    // A deleted label cannot be attached or edited.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task}"),
            &format!(r#"{{"label_ids":["{label}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/labels/{label}"),
            r#"{"name":"x"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    assert_eq!(trash(&fx).await[0]["kind"], "label");
    assert_eq!(restore(&fx, "label", &label).await, StatusCode::NO_CONTENT);
    let t = json(
        request(
            &fx.app,
            get(&format!("/api/tasks/{task}"), Some(&fx.cookie)),
        )
        .await,
    )
    .await;
    assert_eq!(
        t["labels"].as_array().unwrap().len(),
        1,
        "the association survived the soft delete"
    );
    let labels = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/labels", fx.ws),
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(labels[0]["task_count"], 1);

    // Deleting the task drops it from the label's count.
    request(
        &fx.app,
        delete(&format!("/api/tasks/{task}"), Some(&fx.cookie)),
    )
    .await;
    let labels = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/labels", fx.ws),
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(labels[0]["task_count"], 0);
}

#[tokio::test]
async fn trash_needs_write_access() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let reader = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("reader@home.io".to_string()),
        name: Set("Reader".to_string()),
        password_hash: Set("not-a-login-hash".to_string()),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    workspace_members::ActiveModel {
        user_id: Set(reader.id),
        workspace_id: Set(ws),
        role: Set(entity::Role::Read),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    sessions::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        token: Set(token.clone()),
        user_id: Set(reader.id),
        expires_at: Set(crate::common::far_future()),
    }
    .insert(&db)
    .await
    .unwrap();
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{ws}/trash"),
            Some(&format!("woodmouse_session={token}")),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
