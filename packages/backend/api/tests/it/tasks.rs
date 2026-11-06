//! Phase 1 integration tests: projects, lists, tasks, labels — CRUD, access
//! enforcement, assignees, filtering, sorting, and delete cascades.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::{users, workspace_members};
use serde_json::Value;

use crate::common::*;

struct Fixture {
    app: App,
    cookie: String,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    list_id: uuid::Uuid,
}

async fn fixture() -> Fixture {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let workspace_id = create_workspace(&app, &cookie, "Home").await;

    let res = app
        .post(&format!("/api/workspaces/{workspace_id}/projects"))
        .json_text(r#"{"name":"Renovation"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let project_id = json_id(res).await;

    let res = app
        .post(&format!("/api/projects/{project_id}/lists"))
        .json_text(r#"{"name":"Todo"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let list_id = json_id(res).await;

    Fixture {
        app,
        cookie,
        workspace_id,
        project_id,
        list_id,
    }
}

async fn json_id(res: TestResponse) -> uuid::Uuid {
    let body = res.text();
    serde_json::from_str::<Value>(&body)
        .expect("valid json")
        .get("id")
        .expect("id in response")
        .as_str()
        .expect("id as string")
        .parse()
        .expect("valid uuid")
}

async fn create_task(fx: &Fixture, title: &str) -> uuid::Uuid {
    let res = fx
        .app
        .post(&format!("/api/lists/{}/tasks", fx.list_id))
        .json_text(&format!(r#"{{"title":"{title}"}}"#))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    json_id(res).await
}

async fn get_task_json(fx: &Fixture, task_id: uuid::Uuid) -> Value {
    let res = fx
        .app
        .get(&format!("/api/tasks/{task_id}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    serde_json::from_str(&body).expect("valid json")
}

/// Tasks of the fixture's list, via the workspace endpoint. `query` is extra
/// parameters like `?is_completed=true` (or empty).
async fn list_tasks_json(fx: &Fixture, query: &str) -> Vec<Value> {
    let extra = query.trim_start_matches('?');
    let res = fx
        .app
        .get(&format!(
            "/api/workspaces/{}/tasks?list_id={}&{extra}",
            fx.workspace_id, fx.list_id
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    serde_json::from_str(&body).expect("valid json")
}

#[tokio::test]
async fn projects_and_lists_crud() {
    let fx = fixture().await;

    // Projects are listed under the workspace.
    let res = fx
        .app
        .get(&format!("/api/workspaces/{}/projects", fx.workspace_id))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("Renovation"), "body: {body}");

    // Rename the project.
    let res = fx
        .app
        .patch(&format!("/api/projects/{}", fx.project_id))
        .json_text(r#"{"name":"Loft renovation"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("Loft renovation"), "body: {body}");

    // Empty names are rejected.
    let res = fx
        .app
        .post(&format!("/api/workspaces/{}/projects", fx.workspace_id))
        .json_text(r#"{"name":"  "}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    // Lists belong to the project and can be renamed.
    let res = fx
        .app
        .post(&format!("/api/projects/{}/lists", fx.project_id))
        .json_text(r#"{"name":"Doing"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let doing_id = json_id(res).await;

    let res = fx
        .app
        .patch(&format!("/api/lists/{doing_id}"))
        .json_text(r#"{"name":"In progress"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let body = res.text();
    assert!(body.contains("In progress"), "body: {body}");

    // A list of another "project" is not reachable at garbage ids.
    let res = fx
        .app
        .get(&format!("/api/lists/{}", uuid::Uuid::new_v4()))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_lifecycle() {
    let fx = fixture().await;

    let task_a = create_task(&fx, "Buy paint").await;
    let task_b = create_task(&fx, "Sand the floor").await;

    // New tasks append: position increments, defaults apply.
    let a = get_task_json(&fx, task_a).await;
    assert!(a["completed_at"].is_null());
    assert_eq!(a["priority"], "medium");
    assert_eq!(a["position"], 20);
    let b = get_task_json(&fx, task_b).await;
    assert_eq!(b["position"], 40);

    // Partial update: is_completed + priority + due date.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_a}"))
        .json_text(r#"{"is_completed":true,"priority":"urgent","due_date":"2026-01-01T00:00:00Z"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let a = get_task_json(&fx, task_a).await;
    assert!(a["completed_at"].is_string());
    assert_eq!(a["priority"], "urgent");
    assert_eq!(a["due_date"], "2026-01-01T00:00:00Z");

    // Explicit null clears the due date; absent leaves it unchanged.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_a}"))
        .json_text(r#"{"due_date":null}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let a = get_task_json(&fx, task_a).await;
    assert_eq!(a["due_date"], Value::Null);
    assert_eq!(a["priority"], "urgent");

    // Empty title is rejected.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_a}"))
        .json_text(r#"{"title":"  "}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    // Delete removes the task.
    let res = fx
        .app
        .delete(&format!("/api/tasks/{task_a}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = fx
        .app
        .get(&format!("/api/tasks/{task_a}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    let tasks = list_tasks_json(&fx, "").await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["title"], "Sand the floor");
}

#[tokio::test]
async fn assignees_and_labels() {
    let fx = fixture().await;

    // A label in the workspace.
    let res = fx
        .app
        .post(&format!("/api/workspaces/{}/labels", fx.workspace_id))
        .json_text(r##"{"name":"errand","color":"#ff0000"}"##)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let label_id = json_id(res).await;

    // A label in another workspace must not attach.
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Other place").await;
    let res = fx
        .app
        .post(&format!("/api/workspaces/{other_ws}/labels"))
        .json_text(r#"{"name":"foreign"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let foreign_label = json_id(res).await;

    // The admin's user id (needed as an assignee).
    let me: Value = {
        let res = fx.app.get("/api/auth/me").session(Some(&fx.cookie)).await;
        serde_json::from_str(&res.text()).expect("json")
    };
    let admin_id = me["id"].as_str().unwrap();

    // Create with assignee + label.
    let res = fx
        .app
        .post(&format!("/api/lists/{}/tasks", fx.list_id))
        .json_text(&format!(
            r#"{{"title":"Pick up keys","assignee_ids":["{admin_id}"],"label_ids":["{label_id}"]}}"#
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let body = res.text();
    let task: Value = serde_json::from_str(&body).expect("json");
    let task_id = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["assignees"][0]["email"], "admin@home.io");
    assert_eq!(task["labels"][0]["name"], "errand");

    // Replacing the label set works; foreign labels are rejected.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_id}"))
        .json_text(r#"{"label_ids":[]}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let task = get_task_json(&fx, task_id.parse().unwrap()).await;
    assert_eq!(task["labels"].as_array().unwrap().len(), 0);

    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_id}"))
        .json_text(&format!(r#"{{"label_ids":["{foreign_label}"]}}"#))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    // Non-member assignees are rejected.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{task_id}"))
        .json_text(&format!(
            r#"{{"assignee_ids":["{}"]}}"#,
            uuid::Uuid::new_v4()
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);

    // Deleting a label detaches it from tasks.
    let res = fx
        .app
        .post(&format!("/api/lists/{}/tasks", fx.list_id))
        .json_text(&format!(
            r#"{{"title":"Tagged","label_ids":["{label_id}"]}}"#
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let tagged_id = json_id(res).await;

    let res = fx
        .app
        .delete(&format!("/api/labels/{label_id}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let task = get_task_json(&fx, tagged_id).await;
    assert_eq!(task["labels"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn filtering_and_sorting() {
    let fx = fixture().await;

    // Labels and assignee for filters.
    let res = fx
        .app
        .post(&format!("/api/workspaces/{}/labels", fx.workspace_id))
        .json_text(r#"{"name":"errand"}"#)
        .session(Some(&fx.cookie))
        .await;
    let label_id = json_id(res).await;
    let me: Value = {
        let res = fx.app.get("/api/auth/me").session(Some(&fx.cookie)).await;
        serde_json::from_str(&res.text()).expect("json")
    };
    let admin_id = me["id"].as_str().unwrap();

    let t1 = create_task(&fx, "Alpha").await;
    let t2 = create_task(&fx, "Beta").await;
    let t3 = create_task(&fx, "Gamma").await;

    // is_completed filter.
    fx.app
        .patch(&format!("/api/tasks/{t1}"))
        .json_text(r#"{"is_completed":true}"#)
        .session(Some(&fx.cookie))
        .await;
    let done = list_tasks_json(&fx, "?is_completed=true").await;
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["title"], "Alpha");
    let open = list_tasks_json(&fx, "?is_completed=false").await;
    assert_eq!(open.len(), 2);

    // assignee filter (t3 assigned to admin).
    fx.app
        .patch(&format!("/api/tasks/{t3}"))
        .json_text(&format!(r#"{{"assignee_ids":["{admin_id}"]}}"#))
        .session(Some(&fx.cookie))
        .await;
    let assigned = list_tasks_json(&fx, &format!("?assignee_id={admin_id}")).await;
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0]["title"], "Gamma");

    // label filter (t3 labeled).
    fx.app
        .patch(&format!("/api/tasks/{t3}"))
        .json_text(&format!(r#"{{"label_ids":["{label_id}"]}}"#))
        .session(Some(&fx.cookie))
        .await;
    let labeled = list_tasks_json(&fx, &format!("?label_id={label_id}")).await;
    assert_eq!(labeled.len(), 1);
    assert_eq!(labeled[0]["title"], "Gamma");

    // Sorting: title asc/desc; default is position asc.
    let asc = list_tasks_json(&fx, "?sort=title&order=asc").await;
    let titles: Vec<&str> = asc.iter().map(|t| t["title"].as_str().unwrap()).collect();
    assert_eq!(titles, vec!["Alpha", "Beta", "Gamma"]);
    let desc = list_tasks_json(&fx, "?sort=title&order=desc").await;
    let titles: Vec<&str> = desc.iter().map(|t| t["title"].as_str().unwrap()).collect();
    assert_eq!(titles, vec!["Gamma", "Beta", "Alpha"]);

    // due_date sort: tasks without a due date order first ascending (SQLite
    // NULL ordering). Alpha has none; Beta is due before Gamma.
    for (id, due) in [(&t2, "2023-11-14T00:00:00Z"), (&t3, "2027-01-15T00:00:00Z")] {
        fx.app
            .patch(&format!("/api/tasks/{id}"))
            .json_text(&format!(r#"{{"due_date":"{due}"}}"#))
            .session(Some(&fx.cookie))
            .await;
    }
    let by_due = list_tasks_json(&fx, "?sort=due_date&order=asc").await;
    let titles: Vec<&str> = by_due
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Alpha", "Beta", "Gamma"]);

    // Combined filters.
    let combined = list_tasks_json(&fx, "?is_completed=false&sort=title&order=desc").await;
    let titles: Vec<&str> = combined
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Gamma", "Beta"]);
}

#[tokio::test]
async fn access_isolation_and_roles() {
    let (app, db) = app_with_db().await;
    let cookie = bootstrap_admin(&app).await;
    let workspace_id = create_workspace(&app, &cookie, "Home").await;

    let res = app
        .post(&format!("/api/workspaces/{workspace_id}/projects"))
        .json_text(r#"{"name":"Secret project"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let project_id = json_id(res).await;

    // A second, non-member user (seeded directly).
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
    .expect("seed user");
    let other_token = api::sessions::start_session(&db, other.id).await.unwrap();
    let other_cookie = format!("woodmouse_session={other_token}");

    // Non-member: the workspace looks nonexistent (404), nothing is creatable.
    let res = app
        .get(&format!("/api/workspaces/{workspace_id}/projects"))
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    let res = app
        .get(&format!("/api/projects/{project_id}"))
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    let res = app
        .post(&format!("/api/workspaces/{workspace_id}/projects"))
        .json_text(r#"{"name":"Intrude"}"#)
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // As a read-only member: can list, cannot write.
    workspace_members::ActiveModel {
        user_id: Set(other.id),
        workspace_id: Set(workspace_id),
        role: Set(entity::Role::Read),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed membership");

    let res = app
        .get(&format!("/api/workspaces/{workspace_id}/projects"))
        .session(Some(&other_cookie))
        .await;
    let body = res.text();
    assert!(body.contains("Secret project"), "body: {body}");

    let res = app
        .post(&format!("/api/workspaces/{workspace_id}/projects"))
        .json_text(r#"{"name":"Intrude"}"#)
        .session(Some(&other_cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_cascades() {
    let fx = fixture().await;

    let task = create_task(&fx, "Do the thing").await;

    // Labels survive project deletion (they belong to the workspace), but
    // tasks, assignments, and label links do not.
    let res = fx
        .app
        .post(&format!("/api/workspaces/{}/labels", fx.workspace_id))
        .json_text(r#"{"name":"keepme"}"#)
        .session(Some(&fx.cookie))
        .await;
    let label_id = json_id(res).await;
    fx.app
        .patch(&format!("/api/tasks/{task}"))
        .json_text(&format!(r#"{{"label_ids":["{label_id}"]}}"#))
        .session(Some(&fx.cookie))
        .await;

    // Delete the list: its tasks are gone.
    let res = fx
        .app
        .delete(&format!("/api/lists/{}", fx.list_id))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = fx
        .app
        .get(&format!("/api/tasks/{task}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    // Recreate a list in the project and verify the label is still there
    // and attachable — the join rows were cleaned up with the task.
    let res = fx
        .app
        .post(&format!("/api/projects/{}/lists", fx.project_id))
        .json_text(r#"{"name":"Fresh list"}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let fresh_list = json_id(res).await;

    let res = fx
        .app
        .post(&format!("/api/lists/{fresh_list}/tasks"))
        .json_text(&format!(
            r#"{{"title":"New task","label_ids":["{label_id}"]}}"#
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);

    // Delete the project: lists and tasks go with it.
    let res = fx
        .app
        .delete(&format!("/api/projects/{}", fx.project_id))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NO_CONTENT);
    let res = fx
        .app
        .get(&format!("/api/lists/{fresh_list}"))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    let res = fx
        .app
        .get(&format!("/api/workspaces/{}/labels", fx.workspace_id))
        .session(Some(&fx.cookie))
        .await;
    let body = res.text();
    assert!(
        body.contains("keepme"),
        "labels survive project deletion: {body}"
    );
}

#[tokio::test]
async fn move_between_lists_and_start_date() {
    let fx = fixture().await;
    let t1 = create_task(&fx, "Alpha").await;

    let res = fx
        .app
        .post(&format!("/api/projects/{}/lists", fx.project_id))
        .json_text(r#"{"name":"Done"}"#)
        .session(Some(&fx.cookie))
        .await;
    let other_list = json_id(res).await;

    let res = fx
        .app
        .patch(&format!("/api/tasks/{t1}"))
        .json_text(&format!(
            r#"{{"list_id":"{other_list}","start_date":"2023-11-14T00:00:00Z"}}"#
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let task = get_task_json(&fx, t1).await;
    assert_eq!(task["list_id"], other_list.to_string());
    assert_eq!(task["start_date"], "2023-11-14T00:00:00Z");
    assert_eq!(task["position"], 20, "appended to the empty target list");
    assert!(list_tasks_json(&fx, "").await.is_empty());

    // `null` clears the start date.
    fx.app
        .patch(&format!("/api/tasks/{t1}"))
        .json_text(r#"{"start_date":null}"#)
        .session(Some(&fx.cookie))
        .await;
    assert!(get_task_json(&fx, t1).await["start_date"].is_null());

    // Unknown target list: 404.
    let res = fx
        .app
        .patch(&format!("/api/tasks/{t1}"))
        .json_text(&format!(r#"{{"list_id":"{}"}}"#, uuid::Uuid::new_v4()))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_members_listing() {
    let (app, db) = app_with_db().await;
    let cookie = bootstrap_admin(&app).await;
    let workspace_id = create_workspace(&app, &cookie, "Home").await;

    let other = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("other@home.io".to_string()),
        name: Set("Alice".to_string()),
        password_hash: Set(Some("not-a-login-hash".to_string())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed user");
    workspace_members::ActiveModel {
        user_id: Set(other.id),
        workspace_id: Set(workspace_id),
        role: Set(entity::Role::Write),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed membership");

    let res = app
        .get(&format!("/api/workspaces/{workspace_id}/members"))
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    let members: Vec<Value> = serde_json::from_str(&res.text()).expect("json");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["user"]["name"], "Admin", "sorted by name");
    assert_eq!(members[0]["role"], "admin");
    assert_eq!(members[1]["user"]["name"], "Alice");
    assert_eq!(members[1]["role"], "write");
    assert!(members[0]["user"].get("password_hash").is_none());

    // A user with no membership sees a 404, like the workspace itself.
    let stranger = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("stranger@x.io".to_string()),
        name: Set("Stranger".to_string()),
        password_hash: Set(Some("not-a-login-hash".to_string())),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed user");
    let token = api::sessions::start_session(&db, stranger.id)
        .await
        .expect("seed session");
    let res = app
        .get(&format!("/api/workspaces/{workspace_id}/members"))
        .session(Some(&format!("woodmouse_session={token}")))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_task_listing_scopes() {
    let fx = fixture().await;
    create_task(&fx, "In first list").await;

    // A second list in the same project, and a second project with a list.
    let res = fx
        .app
        .post(&format!("/api/projects/{}/lists", fx.project_id))
        .json_text(r#"{"name":"Second"}"#)
        .session(Some(&fx.cookie))
        .await;
    let second_list = json_id(res).await;
    fx.app
        .post(&format!("/api/lists/{second_list}/tasks"))
        .json_text(r#"{"title":"In second list"}"#)
        .session(Some(&fx.cookie))
        .await;

    let res = fx
        .app
        .post(&format!("/api/workspaces/{}/projects", fx.workspace_id))
        .json_text(r#"{"name":"Other project"}"#)
        .session(Some(&fx.cookie))
        .await;
    let other_project = json_id(res).await;
    let res = fx
        .app
        .post(&format!("/api/projects/{other_project}/lists"))
        .json_text(r#"{"name":"Elsewhere"}"#)
        .session(Some(&fx.cookie))
        .await;
    let other_list = json_id(res).await;
    fx.app
        .post(&format!("/api/lists/{other_list}/tasks"))
        .json_text(r#"{"title":"In other project"}"#)
        .session(Some(&fx.cookie))
        .await;

    let fetch = |query: String| {
        let app = fx.app.clone();
        let cookie = fx.cookie.clone();
        let workspace_id = fx.workspace_id;
        async move {
            let res = app
                .get(&format!("/api/workspaces/{workspace_id}/tasks{query}"))
                .session(Some(&cookie))
                .await;
            let status = res.status_code();
            let body = res.text();
            (status, body)
        }
    };
    let count = |body: &str| {
        serde_json::from_str::<Vec<Value>>(body)
            .expect("json")
            .len()
    };

    let (status, body) = fetch(String::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&body), 3, "whole workspace");

    let (_, body) = fetch(format!("?project_id={}", fx.project_id)).await;
    assert_eq!(count(&body), 2, "one project, across its lists");

    let (_, body) = fetch(format!("?list_id={second_list}")).await;
    assert_eq!(count(&body), 1, "one list");

    // A project or list from another workspace (or nonexistent) is a 404.
    let (status, _) = fetch(format!("?project_id={}", uuid::Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fetch(format!("?list_id={second_list}&project_id={other_project}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "list not in that project");

    // Another workspace entirely.
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Work").await;
    let res = fx
        .app
        .get(&format!(
            "/api/workspaces/{other_ws}/tasks?list_id={second_list}"
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn completion_timestamp_and_project_id() {
    let fx = fixture().await;
    let t = create_task(&fx, "Alpha").await;

    let task = get_task_json(&fx, t).await;
    assert!(task["completed_at"].is_null());
    assert!(task["completed_at"].is_null());
    assert_eq!(task["project_id"], fx.project_id.to_string());

    let patch = |body: String| {
        let app = fx.app.clone();
        let cookie = fx.cookie.clone();
        async move {
            let res = app
                .patch(&format!("/api/tasks/{t}"))
                .json_text(&body)
                .session(Some(&cookie))
                .await;
            assert_eq!(res.status_code(), StatusCode::OK);
        }
    };

    // Completing sets the timestamp.
    patch(r#"{"is_completed":true}"#.to_string()).await;
    let done = get_task_json(&fx, t).await;
    assert!(done["completed_at"].is_string());
    let stamp = done["completed_at"].clone();
    assert!(stamp.is_string(), "completed_at set");

    // Completing again (or editing something else) keeps the original timestamp.
    patch(r#"{"is_completed":true,"priority":"high"}"#.to_string()).await;
    assert_eq!(get_task_json(&fx, t).await["completed_at"], stamp);
    patch(r#"{"title":"Alpha 2"}"#.to_string()).await;
    assert_eq!(get_task_json(&fx, t).await["completed_at"], stamp);

    // Reopening while moving to another list (a drag out of Done) clears it.
    let res = fx
        .app
        .post(&format!("/api/projects/{}/lists", fx.project_id))
        .json_text(r#"{"name":"Other"}"#)
        .session(Some(&fx.cookie))
        .await;
    let other = json_id(res).await;
    patch(format!(r#"{{"is_completed":false,"list_id":"{other}"}}"#)).await;
    let reopened = get_task_json(&fx, t).await;
    assert!(reopened["completed_at"].is_null());
    assert_eq!(reopened["list_id"], other.to_string());
}

#[tokio::test]
async fn tasks_without_a_list_are_the_projects_inbox() {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &cookie, "Home").await;
    let project = {
        let res = app
            .post(&format!("/api/workspaces/{ws}/projects"))
            .json_text(r#"{"name":"P"}"#)
            .session(Some(&cookie))
            .await;
        json_id(res).await
    };

    // A new project has no lists; quick adds go to the inbox (no list).
    let res = app
        .get(&format!("/api/projects/{project}/lists"))
        .session(Some(&cookie))
        .await;
    let lists: Vec<Value> = serde_json::from_str(&res.text()).unwrap();
    assert!(lists.is_empty());
    let res = app
        .post(&format!("/api/projects/{project}/tasks"))
        .json_text(r#"{"title":"Quick idea"}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let task: Value = serde_json::from_str(&res.text()).unwrap();
    assert!(task["list_id"].is_null());
    assert_eq!(task["project_id"], project.to_string());
    let task_id = task["id"].as_str().unwrap().to_string();

    // The inbox filter lists them; moving into a list takes it out, and `null` moves it back.
    let inbox = |app: App, cookie: String| async move {
        let res = app
            .get(&format!(
                "/api/workspaces/{ws}/tasks?project_id={project}&inbox=true"
            ))
            .session(Some(&cookie))
            .await;
        serde_json::from_str::<Vec<Value>>(&res.text()).unwrap()
    };
    assert_eq!(inbox(app.clone(), cookie.clone()).await.len(), 1);
    let res = app
        .post(&format!("/api/projects/{project}/lists"))
        .json_text(r#"{"name":"Later"}"#)
        .session(Some(&cookie))
        .await;
    let later = json_id(res).await;
    let res = app
        .patch(&format!("/api/tasks/{task_id}"))
        .json_text(&format!(r#"{{"list_id":"{later}"}}"#))
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert!(inbox(app.clone(), cookie.clone()).await.is_empty());
    let res = app
        .patch(&format!("/api/tasks/{task_id}"))
        .json_text(r#"{"list_id":null}"#)
        .session(Some(&cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::OK);
    assert_eq!(inbox(app.clone(), cookie.clone()).await.len(), 1);
}

#[tokio::test]
async fn scheduling_filters_and_project_links() {
    let fx = fixture().await;
    let day = 86_400;
    let base = 20_833 * 86_400; // a midnight
    let iso = |secs: i64| jiff::Timestamp::from_second(secs).unwrap().to_string();
    let mk = |title: &'static str, extra: String| {
        let (app, cookie, list) = (fx.app.clone(), fx.cookie.clone(), fx.list_id);
        async move {
            let res = app
                .post(&format!("/api/lists/{list}/tasks"))
                .json_text(&format!(r#"{{"title":"{title}"{extra}}}"#))
                .session(Some(&cookie))
                .await;
            json_id(res).await
        }
    };
    let due_only = mk(
        "Due only",
        format!(r#","due_date":"{}""#, iso(base + 5 * day)),
    )
    .await;
    let span = mk(
        "Span",
        format!(
            r#","start_date":"{}","due_date":"{}""#,
            iso(base),
            iso(base + 3 * day)
        ),
    )
    .await;
    let _start_only = mk(
        "Start only",
        format!(r#","start_date":"{}""#, iso(base + 10 * day)),
    )
    .await;
    let _none = mk("Unscheduled", String::new()).await;

    let titles = |query: String| {
        let (app, cookie, ws) = (fx.app.clone(), fx.cookie.clone(), fx.workspace_id);
        async move {
            let res = app
                .get(&format!("/api/workspaces/{ws}/tasks?{query}"))
                .session(Some(&cookie))
                .await;
            let text = res.text();
            let mut t: Vec<String> = serde_json::from_str::<Vec<Value>>(&text)
                .unwrap_or_else(|_| panic!("{query}: {text}"))
                .iter()
                .map(|t| t["title"].as_str().unwrap().to_string())
                .collect();
            t.sort();
            t
        }
    };
    // Overlap, not containment: the span shows up in any window touching it.
    assert_eq!(
        titles(format!(
            "from={}&to={}",
            iso(base + 2 * day),
            iso(base + 4 * day)
        ))
        .await,
        ["Span"]
    );
    assert_eq!(
        titles(format!(
            "from={}&to={}",
            iso(base + 3 * day),
            iso(base + 5 * day)
        ))
        .await,
        ["Due only", "Span"]
    );
    assert_eq!(
        titles(format!("from={}", iso(base + 6 * day))).await,
        ["Start only"]
    );
    assert_eq!(
        titles(format!("to={}", iso(base - day))).await,
        Vec::<String>::new()
    );
    assert_eq!(
        titles("unscheduled=true".to_string()).await,
        ["Unscheduled"]
    );
    assert_eq!(titles(String::new()).await.len(), 4);

    // Links between project tasks, as edges; links to deleted tasks drop out.
    let res = fx
        .app
        .post(&format!("/api/tasks/{span}/links"))
        .json_text(&format!(
            r#"{{"related_task_id":"{due_only}","kind":"blocks"}}"#
        ))
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let edges = |project: String| {
        let (app, cookie, ws) = (fx.app.clone(), fx.cookie.clone(), fx.workspace_id);
        async move {
            let res = app
                .get(&format!("/api/workspaces/{ws}/task-links{project}"))
                .session(Some(&cookie))
                .await;
            assert_eq!(res.status_code(), StatusCode::OK);
            serde_json::from_str::<Vec<Value>>(&res.text()).unwrap()
        }
    };
    let all = edges(String::new()).await;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["task_id"], span.to_string());
    assert_eq!(all[0]["related_task_id"], due_only.to_string());
    assert_eq!(all[0]["kind"], "blocks");
    assert_eq!(
        edges(format!("?project_id={}", fx.project_id)).await.len(),
        1
    );
    assert_eq!(
        edges(format!("?project_id={}", uuid::Uuid::new_v4()))
            .await
            .len(),
        0
    );
    fx.app
        .delete(&format!("/api/tasks/{due_only}"))
        .session(Some(&fx.cookie))
        .await;
    assert!(edges(String::new()).await.is_empty());
}

#[tokio::test]
async fn work_time_is_set_cleared_and_validated() {
    let fx = fixture().await;
    let task = create_task(&fx, "Sand the floor").await;
    let patch = |body: &str| {
        fx.app
            .patch(&format!("/api/tasks/{task}"))
            .json_text(body)
            .session(Some(&fx.cookie))
    };

    let t = get_task_json(&fx, task).await;
    assert!(t["estimate_minutes"].is_null() && t["spent_minutes"].is_null());

    assert_eq!(
        patch(r#"{"estimate_minutes":180,"spent_minutes":45}"#)
            .await
            .status_code(),
        StatusCode::OK
    );
    let t = get_task_json(&fx, task).await;
    assert_eq!(t["estimate_minutes"], 180);
    assert_eq!(t["spent_minutes"], 45);

    // Absent leaves a value alone; null clears it.
    patch(r#"{"title":"Sand it"}"#).await;
    assert_eq!(get_task_json(&fx, task).await["estimate_minutes"], 180);
    patch(r#"{"estimate_minutes":null}"#).await;
    let t = get_task_json(&fx, task).await;
    assert!(t["estimate_minutes"].is_null());
    assert_eq!(t["spent_minutes"], 45);

    // Negative or absurd values are refused.
    assert_eq!(
        patch(r#"{"spent_minutes":-5}"#).await.status_code(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        patch(r#"{"spent_minutes":2000000}"#).await.status_code(),
        StatusCode::BAD_REQUEST
    );

    // New tasks can start with an estimate.
    let res = fx
        .app
        .post(&format!("/api/lists/{}/tasks", fx.list_id))
        .json_text(r#"{"title":"Paint","estimate_minutes":60}"#)
        .session(Some(&fx.cookie))
        .await;
    assert_eq!(res.status_code(), StatusCode::CREATED);
    let created: Value = serde_json::from_str(&res.text()).unwrap();
    assert_eq!(created["estimate_minutes"], 60);
}
