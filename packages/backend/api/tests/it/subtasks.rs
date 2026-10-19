//! Subtasks (any depth) and task links.

use axum::http::StatusCode;
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

impl Fx {
    async fn task(&self, title: &str) -> String {
        make(
            &self.app,
            &self.cookie,
            &format!("/api/lists/{}/tasks", self.list),
            &format!(r#"{{"title":"{title}"}}"#),
        )
        .await
    }
    async fn subtask(&self, parent: &str, title: &str) -> String {
        make(
            &self.app,
            &self.cookie,
            &format!("/api/tasks/{parent}/subtasks"),
            &format!(r#"{{"title":"{title}"}}"#),
        )
        .await
    }
    async fn get(&self, uri: &str) -> Value {
        let res = request(&self.app, get(uri, Some(&self.cookie))).await;
        assert_eq!(res.status(), StatusCode::OK, "GET {uri}");
        json(res).await
    }
    async fn patch(&self, id: &str, body: &str) -> StatusCode {
        request(
            &self.app,
            patch_json(&format!("/api/tasks/{id}"), body, Some(&self.cookie)),
        )
        .await
        .status()
    }
    async fn titles(&self, query: &str) -> Vec<String> {
        self.get(&format!("/api/workspaces/{}/tasks{query}", self.ws))
            .await
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["title"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn nested_subtasks_counts_and_listing() {
    let fx = fixture().await;
    let top = fx.task("Top").await;
    let a = fx.subtask(&top, "A").await;
    let a1 = fx.subtask(&a, "A1").await;
    let a1x = fx.subtask(&a1, "A1x").await; // three levels deep
    let b = fx.subtask(&top, "B").await;

    // Lists hide subtasks by default, and show them on request or by parent.
    assert_eq!(fx.titles("").await, vec!["Top"]);
    assert_eq!(fx.titles("?include_subtasks=true").await.len(), 5);
    let mut direct = fx.titles(&format!("?parent_id={top}")).await;
    direct.sort();
    assert_eq!(direct, vec!["A", "B"]);

    // The whole subtree, flat, with parent ids to rebuild the tree.
    let tree = fx.get(&format!("/api/tasks/{top}/subtasks")).await;
    assert_eq!(tree.as_array().unwrap().len(), 4);
    let deepest = tree
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["title"] == "A1x")
        .unwrap();
    assert_eq!(deepest["parent_id"], a1);
    assert_eq!(deepest["list_id"], fx.list);

    // Counts cover every level.
    let t = fx.get(&format!("/api/tasks/{top}")).await;
    assert_eq!(t["subtask_total"], 4);
    assert_eq!(t["subtask_completed"], 0);
    assert_eq!(
        fx.patch(&a1x, r#"{"is_completed":true}"#).await,
        StatusCode::OK
    );
    assert_eq!(
        fx.patch(&b, r#"{"is_completed":true}"#).await,
        StatusCode::OK
    );
    let t = fx.get(&format!("/api/tasks/{top}")).await;
    assert_eq!(t["subtask_completed"], 2);
    let a_task = fx.get(&format!("/api/tasks/{a}")).await;
    assert_eq!(a_task["subtask_total"], 2);
    assert_eq!(a_task["subtask_completed"], 1);
}

#[tokio::test]
async fn reparenting_and_moving_keep_the_tree_consistent() {
    let fx = fixture().await;
    let top = fx.task("Top").await;
    let a = fx.subtask(&top, "A").await;
    let a1 = fx.subtask(&a, "A1").await;
    let other = fx.task("Other").await;

    // A task cannot go below itself or its own subtasks.
    assert_eq!(
        fx.patch(&top, &format!(r#"{{"parent_id":"{top}"}}"#)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fx.patch(&top, &format!(r#"{{"parent_id":"{a1}"}}"#)).await,
        StatusCode::BAD_REQUEST
    );

    // Move a subtree under another task, then back to top level.
    assert_eq!(
        fx.patch(&a, &format!(r#"{{"parent_id":"{other}"}}"#)).await,
        StatusCode::OK
    );
    let tree = fx.get(&format!("/api/tasks/{other}/subtasks")).await;
    assert_eq!(tree.as_array().unwrap().len(), 2, "A and A1 moved together");
    assert_eq!(
        fx.get(&format!("/api/tasks/{top}")).await["subtask_total"],
        0
    );
    assert_eq!(fx.patch(&a, r#"{"parent_id":null}"#).await, StatusCode::OK);
    assert!(fx.titles("").await.contains(&"A".to_string()));

    // Moving a parent to another list brings its subtasks along; a subtask cannot move alone.
    let list2 = make(
        &fx.app,
        &fx.cookie,
        &format!("/api/projects/{}/lists", fx.project),
        r#"{"name":"L2"}"#,
    )
    .await;
    assert_eq!(
        fx.patch(&a, &format!(r#"{{"list_id":"{list2}"}}"#)).await,
        StatusCode::OK
    );
    assert_eq!(fx.get(&format!("/api/tasks/{a1}")).await["list_id"], list2);
    let sub_of_other = fx.subtask(&other, "Sub").await;
    assert_eq!(
        fx.patch(&sub_of_other, &format!(r#"{{"list_id":"{list2}"}}"#))
            .await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn deleting_and_restoring_a_subtree() {
    let fx = fixture().await;
    let top = fx.task("Top").await;
    let a = fx.subtask(&top, "A").await;
    let _a1 = fx.subtask(&a, "A1").await;

    // Deleting a middle task takes its children; the trash lists only the root.
    let res = request(
        &fx.app,
        delete(&format!("/api/tasks/{a}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fx.get(&format!("/api/tasks/{top}")).await["subtask_total"],
        0
    );
    let trash = fx.get(&format!("/api/workspaces/{}/trash", fx.ws)).await;
    assert_eq!(trash.as_array().unwrap().len(), 1);
    assert_eq!(trash[0]["name"], "A");
    assert_eq!(trash[0]["child_count"], 1);

    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/trash/task/{a}/restore", fx.ws),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fx.get(&format!("/api/tasks/{top}")).await["subtask_total"],
        2
    );

    // Deleting the parent hides everything; a subtask cannot be restored before its parent.
    request(
        &fx.app,
        delete(&format!("/api/tasks/{top}"), Some(&fx.cookie)),
    )
    .await;
    assert!(fx.titles("?include_subtasks=true").await.is_empty());
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/trash/task/{top}/restore", fx.ws),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(fx.titles("?include_subtasks=true").await.len(), 3);
}

#[tokio::test]
async fn task_links() {
    let fx = fixture().await;
    let a = fx.task("A").await;
    let b = fx.task("B").await;

    let res = request(
        &fx.app,
        post_json(
            &format!("/api/tasks/{a}/links"),
            &format!(r#"{{"related_task_id":"{b}","kind":"blocks"}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = json(res).await;
    assert_eq!(created["direction"], "outgoing");
    assert_eq!(created["task"]["title"], "B");
    let link_id = created["id"].as_str().unwrap().to_string();

    // Seen from B it reads the other way round.
    let from_b = fx.get(&format!("/api/tasks/{b}/links")).await;
    assert_eq!(from_b[0]["direction"], "incoming");
    assert_eq!(from_b[0]["kind"], "blocks");
    assert_eq!(from_b[0]["task"]["title"], "A");

    // The same pair and kind, in either direction, is a conflict; another kind is fine.
    for (from, to) in [(&a, &b), (&b, &a)] {
        let res = request(
            &fx.app,
            post_json(
                &format!("/api/tasks/{from}/links"),
                &format!(r#"{{"related_task_id":"{to}","kind":"blocks"}}"#),
                Some(&fx.cookie),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/tasks/{a}/links"),
            &format!(r#"{{"related_task_id":"{b}","kind":"relates_to"}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Self-links are refused; unknown tasks are 404.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/tasks/{a}/links"),
            &format!(r#"{{"related_task_id":"{a}","kind":"blocks"}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/tasks/{a}/links"),
            &format!(
                r#"{{"related_task_id":"{}","kind":"blocks"}}"#,
                uuid::Uuid::new_v4()
            ),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // A link to a deleted task is hidden.
    request(
        &fx.app,
        delete(&format!("/api/tasks/{b}"), Some(&fx.cookie)),
    )
    .await;
    assert!(
        fx.get(&format!("/api/tasks/{a}/links"))
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Removing works from either end.
    let b2 = fx.task("B2").await;
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/tasks/{a}/links"),
            &format!(r#"{{"related_task_id":"{b2}","kind":"duplicates"}}"#),
            Some(&fx.cookie),
        ),
    )
    .await;
    let dup = json(res).await["id"].as_str().unwrap().to_string();
    let res = request(
        &fx.app,
        delete(&format!("/api/tasks/{b2}/links/{dup}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &fx.app,
        delete(&format!("/api/tasks/{a}/links/{link_id}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(
        fx.get(&format!("/api/tasks/{a}/links"))
            .await
            .as_array()
            .unwrap()
            .len()
            <= 1
    );
}
