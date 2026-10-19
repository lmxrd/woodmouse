//! Import and export: CSV of tasks, JSON of chosen parts, previews, and what gets skipped.

use api::db::prelude::*;
use axum::body::Body;
use axum::http::StatusCode;
use entity::{sessions, users, workspace_members};
use serde_json::{Value, json};

use crate::common::*;

struct Fx {
    app: axum::Router,
    db: sea_orm::DatabaseConnection,
    cookie: String,
    ws: uuid::Uuid,
}

async fn json_of(res: axum::http::Response<Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
}

async fn fixture() -> Fx {
    let (app, db) = app_with_db().await;
    let cookie = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &cookie, "Home").await;
    Fx {
        app,
        db,
        cookie,
        ws,
    }
}

impl Fx {
    async fn call(&self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let b = body.to_string();
        let req = match method {
            "POST" => post_json(uri, &b, Some(&self.cookie)),
            "PUT" => put_json(uri, &b, Some(&self.cookie)),
            "PATCH" => patch_json(uri, &b, Some(&self.cookie)),
            _ => get(uri, Some(&self.cookie)),
        };
        let res = request(&self.app, req).await;
        (res.status(), json_of(res).await)
    }
    async fn text(&self, uri: &str) -> (StatusCode, String) {
        let res = request(&self.app, get(uri, Some(&self.cookie))).await;
        (res.status(), body_string(res.into_body()).await)
    }
    /// Import with the given request fields (`format`, `text`, ...); returns the status and the report.
    async fn import_with(&self, cookie: &str, ws: uuid::Uuid, body: Value) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            post_json(
                &format!("/api/workspaces/{ws}/import"),
                &body.to_string(),
                Some(cookie),
            ),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn import(&self, query: &str, text: &str) -> (StatusCode, Value) {
        let mut body = json!({"format": "csv", "text": text});
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap();
            body[k] = match v {
                "true" => json!(true),
                "false" => json!(false),
                _ => json!(v.replace("%20", " ")),
            };
        }
        self.import_with(&self.cookie, self.ws, body).await
    }
    /// A second person who is a member with `role` (and their session cookie and id).
    async fn member(&self, email: &str, role: entity::Role) -> (String, uuid::Uuid) {
        let user = users::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            email: Set(email.to_string()),
            name: Set("".to_string()),
            password_hash: Set("x".to_string()),
            super_admin: Set(false),
            active: Set(true),
            ..Default::default()
        }
        .insert(&self.db)
        .await
        .unwrap();
        workspace_members::ActiveModel {
            user_id: Set(user.id),
            workspace_id: Set(self.ws),
            role: Set(role),
            ..Default::default()
        }
        .insert(&self.db)
        .await
        .unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        sessions::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            token: Set(token.clone()),
            user_id: Set(user.id),
            expires_at: Set(far_future()),
        }
        .insert(&self.db)
        .await
        .unwrap();
        (format!("woodmouse_session={token}"), user.id)
    }
    async fn projects(&self) -> Vec<Value> {
        self.call(
            "GET",
            &format!("/api/workspaces/{}/projects", self.ws),
            Value::Null,
        )
        .await
        .1
        .as_array()
        .unwrap()
        .clone()
    }
    async fn tasks(&self, project: &str) -> Vec<Value> {
        self.call(
            "GET",
            &format!(
                "/api/workspaces/{}/tasks?project_id={project}&include_subtasks=true",
                self.ws
            ),
            Value::Null,
        )
        .await
        .1
        .as_array()
        .unwrap()
        .clone()
    }
}

const CSV: &str = "\
id,parent,project,project_description,list,sprint,sprint_start,sprint_end,title,description,priority,due_date,estimate_minutes,completed,labels,assignees
1,,House,Kitchen first,To do,Sprint 1,2026-10-19,2026-11-01,Order oak,\"Check the, delivery date\",high,2026-10-22,90,,Shopping;Urgent,admin@home.io
2,1,House,,To do,Sprint 1,,,Measure the counter,,,,,,,
3,,House,,Doing,,,,Pick tiles,,urgent,,,true,,
4,,Garden,,,,,,Prune the roses,,,,,,Outdoors,
,,,,,,,,,,,,,,,
";

#[tokio::test]
async fn a_csv_sets_up_projects_lists_sprints_labels_and_tasks() {
    let fx = fixture().await;

    // A preview writes nothing.
    let (status, report) = fx.import("dry_run=true", CSV).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["applied"], false);
    assert_eq!(report["tasks_created"], 4);
    assert_eq!(report["projects_created"], 2);
    assert!(report["problems"].as_array().unwrap().is_empty());
    assert!(fx.projects().await.is_empty());

    let (status, report) = fx.import("", CSV).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["applied"], true);
    assert_eq!(report["tasks_created"], 4);
    assert_eq!(report["lists_created"], 2);
    assert_eq!(report["sprints_created"], 1);
    assert_eq!(report["labels_created"], 3);

    let projects = fx.projects().await;
    assert_eq!(projects.len(), 2);
    let house = projects.iter().find(|p| p["name"] == "House").unwrap();
    assert_eq!(house["description"], "Kitchen first");
    let tasks = fx.tasks(house["id"].as_str().unwrap()).await;
    assert_eq!(tasks.len(), 3);
    let oak = tasks.iter().find(|t| t["title"] == "Order oak").unwrap();
    assert_eq!(oak["description"], "Check the, delivery date");
    assert_eq!(oak["priority"], "high");
    assert_eq!(oak["due_date"], "2026-10-22T00:00:00Z");
    assert_eq!(oak["estimate_minutes"], 90);
    assert_eq!(oak["labels"].as_array().unwrap().len(), 2);
    assert_eq!(oak["assignees"].as_array().unwrap().len(), 1);
    assert!(oak["sprint_id"].is_string());
    let sub = tasks
        .iter()
        .find(|t| t["title"] == "Measure the counter")
        .unwrap();
    assert_eq!(sub["parent_id"], oak["id"]);
    assert_eq!(sub["list_id"], oak["list_id"]);
    let done = tasks.iter().find(|t| t["title"] == "Pick tiles").unwrap();
    assert!(done["completed_at"].is_string());

    // Importing again reuses the projects, lists, sprints and labels by name.
    let (_, again) = fx.import("", CSV).await;
    assert_eq!(again["projects_created"], 0);
    assert_eq!(again["lists_created"], 0);
    assert_eq!(again["labels_created"], 0);
    assert_eq!(again["tasks_created"], 4);
}

#[tokio::test]
async fn an_import_is_everything_or_nothing() {
    let fx = fixture().await;
    let (status, _) = fx.import("", "name,notes\nA,B\n").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a title column is required"
    );
    let (status, _) = fx.import("format=pdf", "x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // One bad row (an orphaned subtask, a bad number, a bad date) stops the whole import.
    let csv = "id,parent,project,title,estimate_minutes,due_date\n\
        1,,P,Fine,30,\n\
        2,9,P,Orphan,,\n\
        3,,P,Bad number,lots,\n\
        4,,P,Bad date,,someday\n";
    let (status, report) = fx.import("", csv).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(report["applied"], false);
    assert_eq!(report["problems"].as_array().unwrap().len(), 3, "{report}");
    assert!(fx.projects().await.is_empty(), "nothing was written");

    // The preview shows the same problems, and fixing the file lets it through.
    let (status, preview) = fx.import("dry_run=true", csv).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["problems"].as_array().unwrap().len(), 3);
    let fixed = "id,parent,project,title,estimate_minutes,due_date\n1,,P,Fine,30,2026-11-02\n2,1,P,Child,,\n";
    let (status, report) = fx.import("", fixed).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["tasks_created"], 2);

    // Rows without a project use the chosen one; with none at all it is a problem.
    let (_, report) = fx.import("project=Inbox%20stuff", "title\nA\n").await;
    assert_eq!(report["applied"], true);
    let (status, report) = fx.import("", "title\nNo project\n").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        report["problems"][0]
            .as_str()
            .unwrap()
            .contains("no project")
    );
}

#[tokio::test]
async fn mapping_to_someone_outside_the_workspace_blocks_everything() {
    let fx = fixture().await;
    // An assignee choice that is not a member passes the preview of the file but cannot be applied.
    let csv = "project,title,assignees\nP,One,\nP,Two,ghost@home.io\n";
    let (status, report) = fx
        .import_with(
            &fx.cookie,
            fx.ws,
            json!({"format": "csv", "text": csv, "user_map": {"ghost@home.io": {"action": "map", "user_id": uuid::Uuid::new_v4()}}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(report["applied"], false);
    assert!(fx.projects().await.is_empty());
}

const PEOPLE_CSV: &str = "project,title,assignees\nP,Order oak,ghost@home.io;admin@home.io\nP,Pick tiles,ghost@home.io;nobody@home.io\n";

#[tokio::test]
async fn people_who_are_not_members_need_a_choice() {
    let fx = fixture().await;
    let (status, report) = fx.import("", PEOPLE_CSV).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let unmapped = report["unmapped_users"].as_array().unwrap();
    assert_eq!(unmapped.len(), 2);
    let ghost = unmapped
        .iter()
        .find(|u| u["email"] == "ghost@home.io")
        .unwrap();
    assert_eq!(ghost["references"], 2);
    assert!(fx.projects().await.is_empty());

    // Map one to a member, skip the other.
    let (cookie, sara) = fx.member("sara@home.io", entity::Role::Write).await;
    let _ = cookie;
    let (status, report) = fx
        .import_with(
            &fx.cookie,
            fx.ws,
            json!({"format": "csv", "text": PEOPLE_CSV, "user_map": {
                "GHOST@home.io": {"action": "map", "user_id": sara},
                "nobody@home.io": {"action": "skip"},
            }}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["applied"], true);
    let project = fx.projects().await[0]["id"].as_str().unwrap().to_string();
    let tasks = fx.tasks(&project).await;
    let oak = tasks.iter().find(|t| t["title"] == "Order oak").unwrap();
    assert_eq!(oak["assignees"].as_array().unwrap().len(), 2);
    let tiles = tasks.iter().find(|t| t["title"] == "Pick tiles").unwrap();
    assert_eq!(
        tiles["assignees"].as_array().unwrap().len(),
        1,
        "the skipped person is left out"
    );
}

#[tokio::test]
async fn an_admin_can_create_the_missing_people_and_others_cannot() {
    let fx = fixture().await;
    let (writer, _) = fx.member("writer@home.io", entity::Role::Write).await;

    // A writer cannot create people.
    let body = json!({"format": "csv", "text": PEOPLE_CSV, "user_map": {
        "ghost@home.io": {"action": "create", "role": "write"},
        "nobody@home.io": {"action": "skip"},
    }});
    let (status, report) = fx.import_with(&writer, fx.ws, body.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        report["problems"][0]
            .as_str()
            .unwrap()
            .contains("workspace admin")
    );
    assert!(fx.projects().await.is_empty());

    // An admin can: the account is created, added to the workspace, and the invite link is returned.
    let (status, report) = fx.import_with(&fx.cookie, fx.ws, body).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["applied"], true);
    assert_eq!(report["invites"].as_array().unwrap().len(), 1);
    assert_eq!(report["invites"][0]["email"], "ghost@home.io");
    let token = report["invites"][0]["invite_token"].as_str().unwrap();
    assert_eq!(
        request(&fx.app, get(&format!("/api/invites/{token}"), None))
            .await
            .status(),
        StatusCode::OK
    );
    let members = fx
        .call(
            "GET",
            &format!("/api/workspaces/{}/members", fx.ws),
            Value::Null,
        )
        .await
        .1;
    assert!(
        members
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["user"]["email"] == "ghost@home.io" && m["role"] == "write")
    );
    let project = fx.projects().await[0]["id"].as_str().unwrap().to_string();
    let oak = fx
        .tasks(&project)
        .await
        .into_iter()
        .find(|t| t["title"] == "Order oak")
        .unwrap();
    assert_eq!(oak["assignees"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn csv_export_and_import_round_trip() {
    let fx = fixture().await;
    fx.import("", CSV).await;
    let (status, csv) = fx
        .text(&format!("/api/workspaces/{}/export/tasks.csv", fx.ws))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(csv.starts_with("id,parent,project,"), "{csv}");
    assert!(csv.contains("\"Check the, delivery date\""));

    // A fresh workspace can be set up from the exported file.
    let other = create_workspace(&fx.app, &fx.cookie, "Copy").await;
    let (status, report) = fx
        .import_with(&fx.cookie, other, json!({"format": "csv", "text": csv}))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["tasks_created"], 4);
    assert_eq!(report["projects_created"], 2);
}

#[tokio::test]
async fn json_exports_chosen_parts_and_imports_them_elsewhere() {
    let fx = fixture().await;
    fx.import("", CSV).await;
    // Wiki, with a nested page and a comment on a task.
    let (_, home) = fx
        .call(
            "POST",
            &format!("/api/workspaces/{}/pages", fx.ws),
            json!({"title": "Household"}),
        )
        .await;
    let home = home["id"].as_str().unwrap().to_string();
    fx.call(
        "PUT",
        &format!("/api/pages/{home}/blocks"),
        json!({"blocks": [{"type": "paragraph", "spans": [{"text": "Welcome"}]}]}),
    )
    .await;
    fx.call(
        "POST",
        &format!("/api/workspaces/{}/pages", fx.ws),
        json!({"title": "Budget", "parent_id": home}),
    )
    .await;
    let house = fx
        .projects()
        .await
        .into_iter()
        .find(|p| p["name"] == "House")
        .unwrap();
    let oak = fx
        .tasks(house["id"].as_str().unwrap())
        .await
        .into_iter()
        .find(|t| t["title"] == "Order oak")
        .unwrap();
    fx.call(
        "POST",
        &format!("/api/tasks/{}/comments", oak["id"].as_str().unwrap()),
        json!({"body": "Back in stock on the 17th."}),
    )
    .await;

    // Only the parts asked for.
    let (status, text) = fx
        .text(&format!(
            "/api/workspaces/{}/export?parts=labels,wiki",
            fx.ws
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["version"], 1);
    assert!(doc["projects"].is_null());
    assert_eq!(doc["labels"].as_array().unwrap().len(), 3);
    assert_eq!(doc["wiki"].as_array().unwrap().len(), 2);
    assert_eq!(doc["wiki"][1]["parent"], doc["wiki"][0]["id"]);
    let (status, _) = fx
        .text(&format!("/api/workspaces/{}/export?parts=nonsense", fx.ws))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // One project, with comments; members are listed for reference.
    let (_, text) = fx
        .text(&format!(
            "/api/workspaces/{}/export?parts=projects,members&project_ids={}",
            fx.ws,
            house["id"].as_str().unwrap()
        ))
        .await;
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["projects"].as_array().unwrap().len(), 1);
    assert_eq!(doc["members"][0]["email"], "admin@home.io");
    assert_eq!(
        doc["projects"][0]["tasks"][0]["comments"][0]["body"],
        "Back in stock on the 17th."
    );

    // Everything, imported into another workspace.
    let (_, everything) = fx
        .text(&format!(
            "/api/workspaces/{}/export?parts=projects,labels,wiki,members",
            fx.ws
        ))
        .await;
    let other = create_workspace(&fx.app, &fx.cookie, "Copy").await;
    let body = |dry: bool| json!({"format": "json", "text": everything, "dry_run": dry});
    let (_, preview) = fx.import_with(&fx.cookie, other, body(true)).await;
    assert_eq!(preview["tasks_created"], 4);
    assert_eq!(preview["pages_created"], 2);
    let (status, report) = fx.import_with(&fx.cookie, other, body(false)).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["tasks_created"], 4);
    assert_eq!(report["comments_created"], 1);
    assert_eq!(report["pages_created"], 2);
    let pages = json_of(
        request(
            &fx.app,
            get(&format!("/api/workspaces/{other}/pages"), Some(&fx.cookie)),
        )
        .await,
    )
    .await;
    assert_eq!(pages.as_array().unwrap().len(), 2);

    // Not a version we know.
    let (status, _) = fx
        .import_with(
            &fx.cookie,
            other,
            json!({"format": "json", "text": r#"{"version": 99}"#}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
