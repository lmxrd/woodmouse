//! The activity feed: what gets recorded, and the workspace / project / task scopes.

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

async fn id_of(app: &axum::Router, cookie: &str, uri: &str, body: &str) -> String {
    let res = request(app, post_json(uri, body, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::CREATED, "POST {uri}");
    json(res).await["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn records_what_happens_and_scopes_the_feed() {
    let app = app().await;
    let c = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &c, "Home").await;
    let me = json(request(&app, get("/api/auth/me", Some(&c))).await).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let project = id_of(
        &app,
        &c,
        &format!("/api/workspaces/{ws}/projects"),
        r#"{"name":"Kitchen"}"#,
    )
    .await;
    let todo = id_of(
        &app,
        &c,
        &format!("/api/projects/{project}/lists"),
        r#"{"name":"To do"}"#,
    )
    .await;
    let doing = id_of(
        &app,
        &c,
        &format!("/api/projects/{project}/lists"),
        r#"{"name":"Doing"}"#,
    )
    .await;
    let task = id_of(
        &app,
        &c,
        &format!("/api/lists/{todo}/tasks"),
        r#"{"title":"Order worktop"}"#,
    )
    .await;
    let _sub = id_of(
        &app,
        &c,
        &format!("/api/tasks/{task}/subtasks"),
        r#"{"title":"Measure"}"#,
    )
    .await;

    let patch = |body: String| {
        let app = app.clone();
        let c = c.clone();
        let task = task.clone();
        async move {
            let res = request(
                &app,
                patch_json(&format!("/api/tasks/{task}"), &body, Some(&c)),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
        }
    };
    patch(r#"{"title":"Order oak worktop"}"#.to_string()).await;
    patch(format!(r#"{{"list_id":"{doing}"}}"#)).await;
    patch(r#"{"is_completed":true}"#.to_string()).await;
    patch(r#"{"is_completed":false,"priority":"high","due_date":1800000000}"#.to_string()).await;
    patch(format!(r#"{{"assignee_ids":["{me}"]}}"#)).await;
    patch(r#"{"assignee_ids":[]}"#.to_string()).await;
    patch(r#"{"position":9.5}"#.to_string()).await; // no feed-worthy change

    id_of(
        &app,
        &c,
        &format!("/api/tasks/{task}/comments"),
        r#"{"body":"Back in stock on the 17th."}"#,
    )
    .await;
    let other = id_of(
        &app,
        &c,
        &format!("/api/lists/{todo}/tasks"),
        r#"{"title":"Pick tile"}"#,
    )
    .await;
    id_of(
        &app,
        &c,
        &format!("/api/tasks/{task}/links"),
        &format!(r#"{{"related_task_id":"{other}","kind":"blocks"}}"#),
    )
    .await;
    let res = request(
        &app,
        upload(
            &format!("/api/workspaces/{ws}/files"),
            "quote.pdf",
            "application/pdf",
            b"pdf",
            Some(&task),
            Some(&c),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    request(&app, delete(&format!("/api/tasks/{task}"), Some(&c))).await;
    request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/trash/task/{task}/restore"),
            "{}",
            Some(&c),
        ),
    )
    .await;

    let feed = |query: String| {
        let app = app.clone();
        let c = c.clone();
        async move {
            let res = request(
                &app,
                get(&format!("/api/workspaces/{ws}/activity{query}"), Some(&c)),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
            json(res).await.as_array().unwrap().clone()
        }
    };
    let kinds = |entries: &[Value]| -> Vec<String> {
        entries
            .iter()
            .rev()
            .map(|e| e["kind"].as_str().unwrap().to_string())
            .collect()
    };

    // The task feed, oldest first for easy reading.
    let task_feed = feed(format!("?task_id={task}")).await;
    assert_eq!(
        kinds(&task_feed),
        [
            "task_created",
            "subtask_added",
            "task_renamed",
            "task_moved",
            "task_completed",
            "task_reopened",
            "due_changed",
            "priority_changed",
            "assigned",
            "unassigned",
            "comment_added",
            "link_added",
            "file_attached",
            "task_deleted",
            "task_restored",
        ]
        .map(String::from)
        .to_vec()
    );
    // Entries carry the actor and snapshots for phrasing.
    let renamed = task_feed
        .iter()
        .find(|e| e["kind"] == "task_renamed")
        .unwrap();
    assert_eq!(renamed["data"]["from"], "Order worktop");
    assert_eq!(renamed["data"]["to"], "Order oak worktop");
    assert_eq!(renamed["actor"]["id"], me);
    let moved = task_feed
        .iter()
        .find(|e| e["kind"] == "task_moved")
        .unwrap();
    assert_eq!(
        (moved["data"]["from"].as_str(), moved["data"]["to"].as_str()),
        (Some("To do"), Some("Doing"))
    );
    let attached = task_feed
        .iter()
        .find(|e| e["kind"] == "file_attached")
        .unwrap();
    assert_eq!(attached["data"]["file"], "quote.pdf");

    // Project and workspace feeds include more (the second task's creation).
    let project_feed = feed(format!("?project_id={project}")).await;
    assert_eq!(project_feed.len(), task_feed.len() + 1);
    assert_eq!(feed(String::new()).await.len(), project_feed.len());

    // Paging: limit and before.
    let first_page = feed("?limit=3".to_string()).await;
    assert_eq!(first_page.len(), 3);
    let cursor = first_page.last().unwrap()["created_at"].as_i64().unwrap();
    let older = feed(format!("?before={cursor}&limit=200")).await;
    assert!(
        older
            .iter()
            .all(|e| e["created_at"].as_i64().unwrap() < cursor)
    );

    // Another workspace never sees these entries, even when asking for this project or task.
    let other_ws = create_workspace(&app, &c, "Work").await;
    for query in [
        format!("?task_id={task}"),
        format!("?project_id={project}"),
        String::new(),
    ] {
        let res = request(
            &app,
            get(
                &format!("/api/workspaces/{other_ws}/activity{query}"),
                Some(&c),
            ),
        )
        .await;
        assert!(json(res).await.as_array().unwrap().is_empty());
    }
    let res = request(&app, get(&format!("/api/workspaces/{ws}/activity"), None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
