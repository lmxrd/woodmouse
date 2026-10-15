//! Phase 1 integration tests: projects, lists, tasks, labels — CRUD, access
//! enforcement, assignees, filtering, sorting, and delete cascades.

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::*;

struct Fixture {
    app: axum::Router,
    cookie: String,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    list_id: uuid::Uuid,
}

async fn fixture() -> Fixture {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let workspace_id = create_workspace(&app, &cookie, "Home").await;

    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{workspace_id}/projects"),
            r#"{"name":"Renovation"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let project_id = json_id(res).await;

    let res = request(
        &app,
        post_json(
            &format!("/api/projects/{project_id}/lists"),
            r#"{"name":"Todo"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let list_id = json_id(res).await;

    Fixture {
        app,
        cookie,
        workspace_id,
        project_id,
        list_id,
    }
}

async fn json_id(res: axum::http::Response<axum::body::Body>) -> uuid::Uuid {
    let body = body_string(res.into_body()).await;
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
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/lists/{}/tasks", fx.list_id),
            &format!(r#"{{"title":"{title}"}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_id(res).await
}

async fn get_task_json(fx: &Fixture, task_id: uuid::Uuid) -> Value {
    let res = request(
        &fx.app,
        get(&format!("/api/tasks/{task_id}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    serde_json::from_str(&body).expect("valid json")
}

/// Tasks of the fixture's list, via the workspace endpoint. `query` is extra
/// parameters like `?is_completed=true` (or empty).
async fn list_tasks_json(fx: &Fixture, query: &str) -> Vec<Value> {
    let extra = query.trim_start_matches('?');
    let res = request(
        &fx.app,
        get(
            &format!(
                "/api/workspaces/{}/tasks?list_id={}&{extra}",
                fx.workspace_id, fx.list_id
            ),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    serde_json::from_str(&body).expect("valid json")
}

#[tokio::test]
async fn projects_and_lists_crud() {
    let fx = fixture().await;

    // Projects are listed under the workspace.
    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{}/projects", fx.workspace_id),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("Renovation"), "body: {body}");

    // Rename the project.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/projects/{}", fx.project_id),
            r#"{"name":"Loft renovation"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("Loft renovation"), "body: {body}");

    // Empty names are rejected.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/projects", fx.workspace_id),
            r#"{"name":"  "}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Lists belong to the project and can be renamed.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{}/lists", fx.project_id),
            r#"{"name":"Doing"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let doing_id = json_id(res).await;

    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/lists/{doing_id}"),
            r#"{"name":"In progress"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("In progress"), "body: {body}");

    // A list of another "project" is not reachable at garbage ids.
    let res = request(
        &fx.app,
        get(
            &format!("/api/lists/{}", uuid::Uuid::new_v4()),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_lifecycle() {
    let fx = fixture().await;

    let task_a = create_task(&fx, "Buy paint").await;
    let task_b = create_task(&fx, "Sand the floor").await;

    // New tasks append: position increments, defaults apply.
    let a = get_task_json(&fx, task_a).await;
    assert_eq!(a["is_completed"], false);
    assert_eq!(a["priority"], "medium");
    assert_eq!(a["position"], 1.0);
    let b = get_task_json(&fx, task_b).await;
    assert_eq!(b["position"], 2.0);

    // Partial update: is_completed + priority + due date.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_a}"),
            r#"{"is_completed":true,"priority":"urgent","due_date":1767225600}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let a = get_task_json(&fx, task_a).await;
    assert_eq!(a["is_completed"], true);
    assert_eq!(a["priority"], "urgent");
    assert_eq!(a["due_date"], 1767225600);

    // Explicit null clears the due date; absent leaves it unchanged.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_a}"),
            r#"{"due_date":null}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let a = get_task_json(&fx, task_a).await;
    assert_eq!(a["due_date"], Value::Null);
    assert_eq!(a["priority"], "urgent");

    // Empty title is rejected.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_a}"),
            r#"{"title":"  "}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Delete removes the task.
    let res = request(
        &fx.app,
        delete(&format!("/api/tasks/{task_a}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &fx.app,
        get(&format!("/api/tasks/{task_a}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let tasks = list_tasks_json(&fx, "").await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["title"], "Sand the floor");
}

#[tokio::test]
async fn assignees_and_labels() {
    let fx = fixture().await;

    // A label in the workspace.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/labels", fx.workspace_id),
            r##"{"name":"errand","color":"#ff0000"}"##,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let label_id = json_id(res).await;

    // A label in another workspace must not attach.
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Other place").await;
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{other_ws}/labels"),
            r#"{"name":"foreign"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let foreign_label = json_id(res).await;

    // The admin's user id (needed as an assignee).
    let me: Value = {
        let res = request(&fx.app, get("/api/auth/me", Some(&fx.cookie))).await;
        serde_json::from_str(&body_string(res.into_body()).await).expect("json")
    };
    let admin_id = me["id"].as_str().unwrap();

    // Create with assignee + label.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/lists/{}/tasks", fx.list_id),
            &format!(
                r#"{{"title":"Pick up keys","assignee_ids":["{admin_id}"],"label_ids":["{label_id}"]}}"#
            ),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_string(res.into_body()).await;
    let task: Value = serde_json::from_str(&body).expect("json");
    let task_id = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["assignees"][0]["email"], "admin@home.io");
    assert_eq!(task["labels"][0]["name"], "errand");

    // Replacing the label set works; foreign labels are rejected.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_id}"),
            r#"{"label_ids":[]}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let task = get_task_json(&fx, task_id.parse().unwrap()).await;
    assert_eq!(task["labels"].as_array().unwrap().len(), 0);

    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_id}"),
            &format!(r#"{{"label_ids":["{foreign_label}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Non-member assignees are rejected.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_id}"),
            &format!(r#"{{"assignee_ids":["{}"]}}"#, uuid::Uuid::new_v4()),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Deleting a label detaches it from tasks.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/lists/{}/tasks", fx.list_id),
            &format!(r#"{{"title":"Tagged","label_ids":["{label_id}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let tagged_id = json_id(res).await;

    let res = request(
        &fx.app,
        delete(&format!("/api/labels/{label_id}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let task = get_task_json(&fx, tagged_id).await;
    assert_eq!(task["labels"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn filtering_and_sorting() {
    let fx = fixture().await;

    // Labels and assignee for filters.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/labels", fx.workspace_id),
            r#"{"name":"errand"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let label_id = json_id(res).await;
    let me: Value = {
        let res = request(&fx.app, get("/api/auth/me", Some(&fx.cookie))).await;
        serde_json::from_str(&body_string(res.into_body()).await).expect("json")
    };
    let admin_id = me["id"].as_str().unwrap();

    let t1 = create_task(&fx, "Alpha").await;
    let t2 = create_task(&fx, "Beta").await;
    let t3 = create_task(&fx, "Gamma").await;

    // is_completed filter.
    request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t1}"),
            r#"{"is_completed":true}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let done = list_tasks_json(&fx, "?is_completed=true").await;
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["title"], "Alpha");
    let open = list_tasks_json(&fx, "?is_completed=false").await;
    assert_eq!(open.len(), 2);

    // assignee filter (t3 assigned to admin).
    request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t3}"),
            &format!(r#"{{"assignee_ids":["{admin_id}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    let assigned = list_tasks_json(&fx, &format!("?assignee_id={admin_id}")).await;
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0]["title"], "Gamma");

    // label filter (t3 labeled).
    request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t3}"),
            &format!(r#"{{"label_ids":["{label_id}"]}}"#),
            Some(&fx.cookie),
        ),
    )
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
    for (id, due) in [(&t2, 1700000000), (&t3, 1800000000)] {
        request(
            &fx.app,
            patch_json(
                &format!("/api/tasks/{id}"),
                &format!(r#"{{"due_date":{due}}}"#),
                Some(&fx.cookie),
            ),
        )
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

    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{workspace_id}/projects"),
            r#"{"name":"Secret project"}"#,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let project_id = json_id(res).await;

    // A second, non-member user (seeded directly).
    let other = toasty::create!(woodmouse::models::User {
        email: "other@home.io".to_string(),
        name: "Other".to_string(),
        password_hash: "not-a-login-hash".to_string(),
        super_admin: false,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed user");
    let other_token = uuid::Uuid::new_v4().to_string();
    toasty::create!(woodmouse::models::Session {
        token: other_token.clone(),
        user_id: other.id,
        expires_at: 4_102_444_800,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed session");
    let other_cookie = format!("woodmouse_session={other_token}");

    // Non-member: the workspace looks nonexistent (404), nothing is creatable.
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}/projects"),
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = request(
        &app,
        get(&format!("/api/projects/{project_id}"), Some(&other_cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{workspace_id}/projects"),
            r#"{"name":"Intrude"}"#,
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // As a read-only member: can list, cannot write.
    toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: other.id,
        workspace_id,
        role: woodmouse::models::Role::Read,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed membership");

    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}/projects"),
            Some(&other_cookie),
        ),
    )
    .await;
    let body = body_string(res.into_body()).await;
    assert!(body.contains("Secret project"), "body: {body}");

    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{workspace_id}/projects"),
            r#"{"name":"Intrude"}"#,
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_cascades() {
    let fx = fixture().await;

    let task = create_task(&fx, "Do the thing").await;

    // Labels survive project deletion (they belong to the workspace), but
    // tasks, assignments, and label links do not.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/labels", fx.workspace_id),
            r#"{"name":"keepme"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let label_id = json_id(res).await;
    request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task}"),
            &format!(r#"{{"label_ids":["{label_id}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;

    // Delete the list: its tasks are gone.
    let res = request(
        &fx.app,
        delete(&format!("/api/lists/{}", fx.list_id), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &fx.app,
        get(&format!("/api/tasks/{task}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Recreate a list in the project and verify the label is still there
    // and attachable — the join rows were cleaned up with the task.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{}/lists", fx.project_id),
            r#"{"name":"Fresh list"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let fresh_list = json_id(res).await;

    let res = request(
        &fx.app,
        post_json(
            &format!("/api/lists/{fresh_list}/tasks"),
            &format!(r#"{{"title":"New task","label_ids":["{label_id}"]}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Delete the project: lists and tasks go with it.
    let res = request(
        &fx.app,
        delete(
            &format!("/api/projects/{}", fx.project_id),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &fx.app,
        get(&format!("/api/lists/{fresh_list}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{}/labels", fx.workspace_id),
            Some(&fx.cookie),
        ),
    )
    .await;
    let body = body_string(res.into_body()).await;
    assert!(
        body.contains("keepme"),
        "labels survive project deletion: {body}"
    );
}

#[tokio::test]
async fn move_between_lists_and_start_date() {
    let fx = fixture().await;
    let t1 = create_task(&fx, "Alpha").await;

    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{}/lists", fx.project_id),
            r#"{"name":"Done"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let other_list = json_id(res).await;

    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t1}"),
            &format!(r#"{{"list_id":"{other_list}","start_date":1700000000}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let task = get_task_json(&fx, t1).await;
    assert_eq!(task["list_id"], other_list.to_string());
    assert_eq!(task["start_date"], 1700000000);
    assert_eq!(task["position"], 1.0, "appended to the empty target list");
    assert!(list_tasks_json(&fx, "").await.is_empty());

    // `null` clears the start date.
    request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t1}"),
            r#"{"start_date":null}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert!(get_task_json(&fx, t1).await["start_date"].is_null());

    // Unknown target list: 404.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{t1}"),
            &format!(r#"{{"list_id":"{}"}}"#, uuid::Uuid::new_v4()),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_members_listing() {
    let (app, db) = app_with_db().await;
    let cookie = bootstrap_admin(&app).await;
    let workspace_id = create_workspace(&app, &cookie, "Home").await;

    let other = toasty::create!(woodmouse::models::User {
        email: "other@home.io".to_string(),
        name: "Alice".to_string(),
        password_hash: "not-a-login-hash".to_string(),
        super_admin: false,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed user");
    toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: other.id,
        workspace_id,
        role: woodmouse::models::Role::Write,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed membership");

    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}/members"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let members: Vec<Value> =
        serde_json::from_str(&body_string(res.into_body()).await).expect("json");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["user"]["name"], "Admin", "sorted by name");
    assert_eq!(members[0]["role"], "admin");
    assert_eq!(members[1]["user"]["name"], "Alice");
    assert_eq!(members[1]["role"], "write");
    assert!(members[0]["user"].get("password_hash").is_none());

    // A user with no membership sees a 404, like the workspace itself.
    let token = uuid::Uuid::new_v4().to_string();
    let stranger = toasty::create!(woodmouse::models::User {
        email: "stranger@x.io".to_string(),
        name: "Stranger".to_string(),
        password_hash: "not-a-login-hash".to_string(),
        super_admin: false,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed user");
    toasty::create!(woodmouse::models::Session {
        token: token.clone(),
        user_id: stranger.id,
        expires_at: 4_102_444_800,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed session");
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}/members"),
            Some(&format!("woodmouse_session={token}")),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_task_listing_scopes() {
    let fx = fixture().await;
    create_task(&fx, "In first list").await;

    // A second list in the same project, and a second project with a list.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{}/lists", fx.project_id),
            r#"{"name":"Second"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let second_list = json_id(res).await;
    request(
        &fx.app,
        post_json(
            &format!("/api/lists/{second_list}/tasks"),
            r#"{"title":"In second list"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;

    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/projects", fx.workspace_id),
            r#"{"name":"Other project"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let other_project = json_id(res).await;
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{other_project}/lists"),
            r#"{"name":"Elsewhere"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let other_list = json_id(res).await;
    request(
        &fx.app,
        post_json(
            &format!("/api/lists/{other_list}/tasks"),
            r#"{"title":"In other project"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;

    let fetch = |query: String| {
        let app = fx.app.clone();
        let cookie = fx.cookie.clone();
        let workspace_id = fx.workspace_id;
        async move {
            let res = request(
                &app,
                get(
                    &format!("/api/workspaces/{workspace_id}/tasks{query}"),
                    Some(&cookie),
                ),
            )
            .await;
            let status = res.status();
            let body = body_string(res.into_body()).await;
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
    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{other_ws}/tasks?list_id={second_list}"),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn completion_timestamp_and_project_id() {
    let fx = fixture().await;
    let t = create_task(&fx, "Alpha").await;

    let task = get_task_json(&fx, t).await;
    assert_eq!(task["is_completed"], false);
    assert!(task["completed_at"].is_null());
    assert_eq!(task["project_id"], fx.project_id.to_string());

    let patch = |body: String| {
        let app = fx.app.clone();
        let cookie = fx.cookie.clone();
        async move {
            let res = request(
                &app,
                patch_json(&format!("/api/tasks/{t}"), &body, Some(&cookie)),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
        }
    };

    // Completing sets the timestamp.
    patch(r#"{"is_completed":true}"#.to_string()).await;
    let done = get_task_json(&fx, t).await;
    assert_eq!(done["is_completed"], true);
    let stamp = done["completed_at"].as_i64().expect("completed_at set");
    assert!(stamp > 1_700_000_000);

    // Completing again (or editing something else) keeps the original timestamp.
    patch(r#"{"is_completed":true,"priority":"high"}"#.to_string()).await;
    assert_eq!(get_task_json(&fx, t).await["completed_at"], stamp);
    patch(r#"{"title":"Alpha 2"}"#.to_string()).await;
    assert_eq!(get_task_json(&fx, t).await["completed_at"], stamp);

    // Reopening while moving to another list (a drag out of Done) clears it.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/projects/{}/lists", fx.project_id),
            r#"{"name":"Other"}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    let other = json_id(res).await;
    patch(format!(r#"{{"is_completed":false,"list_id":"{other}"}}"#)).await;
    let reopened = get_task_json(&fx, t).await;
    assert_eq!(reopened["is_completed"], false);
    assert!(reopened["completed_at"].is_null());
    assert_eq!(reopened["list_id"], other.to_string());
}
