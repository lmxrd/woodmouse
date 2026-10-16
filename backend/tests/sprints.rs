//! Sprints: dated buckets of a project's tasks, completing them, and how they interact with tasks.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::*;

struct Fx {
    app: axum::Router,
    cookie: String,
    ws: uuid::Uuid,
    project: String,
    other_project: String,
}

async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
}

async fn fixture() -> Fx {
    let app = app().await;
    let cookie = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &cookie, "Home").await;
    let mut ids = vec![];
    for name in ["P", "Q"] {
        let res = request(
            &app,
            post_json(
                &format!("/api/workspaces/{ws}/projects"),
                &format!(r#"{{"name":"{name}"}}"#),
                Some(&cookie),
            ),
        )
        .await;
        ids.push(json_of(res).await["id"].as_str().unwrap().to_string());
    }
    Fx {
        app,
        cookie,
        ws,
        project: ids[0].clone(),
        other_project: ids[1].clone(),
    }
}

impl Fx {
    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            post_json(uri, &body.to_string(), Some(&self.cookie)),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn patch(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            patch_json(uri, &body.to_string(), Some(&self.cookie)),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn get(&self, uri: &str) -> Value {
        json_of(request(&self.app, get(uri, Some(&self.cookie))).await).await
    }
    async fn count(&self, query: &str) -> usize {
        let uri = format!(
            "/api/workspaces/{}/tasks?project_id={}&{query}",
            self.ws, self.project
        );
        self.get(&uri).await.as_array().unwrap().len()
    }
    async fn sprint(&self, project: &str, name: &str) -> String {
        let (status, s) = self
            .post(
                &format!("/api/projects/{project}/sprints"),
                json!({"name": name, "start_date": "2026-10-19T00:00:00Z", "end_date": "2026-11-01T00:00:00Z"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        s["id"].as_str().unwrap().to_string()
    }
    async fn task(&self, title: &str, extra: Value) -> String {
        let mut body = json!({ "title": title });
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, t) = self
            .post(&format!("/api/projects/{}/tasks", self.project), body)
            .await;
        assert_eq!(status, StatusCode::CREATED);
        t["id"].as_str().unwrap().to_string()
    }
}

#[tokio::test]
async fn sprints_hold_tasks_and_summarise_them() {
    let fx = fixture().await;
    let sprint = fx.sprint(&fx.project, "Sprint 1").await;

    let a = fx
        .task("A", json!({"sprint_id": sprint, "estimate_minutes": 60}))
        .await;
    fx.task("B", json!({"sprint_id": sprint, "estimate_minutes": 90}))
        .await;
    fx.task("Loose", json!({})).await;
    fx.patch(&format!("/api/tasks/{a}"), json!({"is_completed": true}))
        .await;

    let list = fx
        .get(&format!("/api/projects/{}/sprints", fx.project))
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["task_count"], 2);
    assert_eq!(list[0]["done_count"], 1);
    assert_eq!(list[0]["estimate_minutes"], 150);

    // The task list filters by sprint and by backlog.
    assert_eq!(fx.count(&format!("sprint_id={sprint}")).await, 2);
    assert_eq!(fx.count("no_sprint=true").await, 1);

    // A task can leave its sprint (null) and rejoin it.
    let (_, t) = fx
        .patch(&format!("/api/tasks/{a}"), json!({"sprint_id": null}))
        .await;
    assert!(t["sprint_id"].is_null());
    let (_, t) = fx
        .patch(&format!("/api/tasks/{a}"), json!({"sprint_id": sprint}))
        .await;
    assert_eq!(t["sprint_id"], sprint);
}

#[tokio::test]
async fn sprints_validate_their_dates_and_their_project() {
    let fx = fixture().await;
    let (status, _) = fx
        .post(
            &format!("/api/projects/{}/sprints", fx.project),
            json!({"name": "Backwards", "start_date": "2026-11-01T00:00:00Z", "end_date": "2026-10-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = fx
        .post(
            &format!("/api/projects/{}/sprints", fx.project),
            json!({"name": " ", "start_date": "2026-10-01T00:00:00Z", "end_date": "2026-11-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A task cannot be planned in another project's sprint.
    let foreign = fx.sprint(&fx.other_project, "Elsewhere").await;
    let t = fx.task("A", json!({})).await;
    let (status, _) = fx
        .patch(&format!("/api/tasks/{t}"), json!({"sprint_id": foreign}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let sprint = fx.sprint(&fx.project, "Sprint 1").await;
    let (status, s) = fx
        .patch(
            &format!("/api/sprints/{sprint}"),
            json!({"name": "Sprint one", "end_date": "2026-11-08T00:00:00Z"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s["name"], "Sprint one");
    assert_eq!(s["end_date"], "2026-11-08T00:00:00Z");
}

#[tokio::test]
async fn completing_a_sprint_moves_unfinished_tasks_on() {
    let fx = fixture().await;
    let one = fx.sprint(&fx.project, "One").await;
    let two = fx.sprint(&fx.project, "Two").await;
    let done = fx.task("Done", json!({"sprint_id": one})).await;
    let open = fx.task("Open", json!({"sprint_id": one})).await;
    fx.patch(&format!("/api/tasks/{done}"), json!({"is_completed": true}))
        .await;

    let (status, s) = fx
        .post(
            &format!("/api/sprints/{one}/complete"),
            json!({"move_to": two}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(s["completed_at"].is_string());
    assert_eq!(
        fx.get(&format!("/api/tasks/{done}")).await["sprint_id"],
        one,
        "finished work stays as the record"
    );
    assert_eq!(
        fx.get(&format!("/api/tasks/{open}")).await["sprint_id"],
        two
    );

    // It cannot be completed twice, and a completed sprint takes no more tasks.
    let (status, _) = fx
        .post(&format!("/api/sprints/{one}/complete"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = fx
        .patch(&format!("/api/tasks/{open}"), json!({"sprint_id": one}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Completing the second sends the rest to the backlog.
    fx.post(&format!("/api/sprints/{two}/complete"), json!({}))
        .await;
    assert!(fx.get(&format!("/api/tasks/{open}")).await["sprint_id"].is_null());
}

#[tokio::test]
async fn deleting_a_sprint_returns_its_tasks_to_the_backlog_and_can_be_restored() {
    let fx = fixture().await;
    let sprint = fx.sprint(&fx.project, "Doomed").await;
    let t = fx.task("A", json!({"sprint_id": sprint})).await;

    let res = request(
        &fx.app,
        delete(&format!("/api/sprints/{sprint}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(fx.get(&format!("/api/tasks/{t}")).await["sprint_id"].is_null());
    let list = fx
        .get(&format!("/api/projects/{}/sprints", fx.project))
        .await;
    assert!(list.as_array().unwrap().is_empty());

    let trash = fx.get(&format!("/api/workspaces/{}/trash", fx.ws)).await;
    let item = trash
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "sprint")
        .expect("the sprint is in the trash");
    assert_eq!(item["name"], "Doomed");
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/trash/sprint/{sprint}/restore", fx.ws),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let list = fx
        .get(&format!("/api/projects/{}/sprints", fx.project))
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        list[0]["task_count"], 0,
        "restoring does not put the tasks back"
    );
}
