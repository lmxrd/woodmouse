//! Mock data for development: `woodmouse seed` fills an empty database with two people, a workspace with
//! projects, tasks, comments and wiki pages. It goes through the real API (in process), so everything
//! that normally accompanies a change (revisions, the activity feed, search) is there too.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::time;

/// The password of every seeded account.
pub const PASSWORD: &str = "correct horse battery";

struct Api {
    router: Router,
}

impl Api {
    async fn call(
        &self,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Value,
    ) -> (Value, Option<String>) {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/api{uri}"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let request = request.body(Body::from(body.to_string())).expect("request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_string);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(
            status.is_success(),
            "{method} {uri} failed with {status}: {value}"
        );
        (value, set_cookie)
    }

    async fn post(&self, uri: &str, cookie: &str, body: Value) -> Value {
        self.call("POST", uri, Some(cookie), body).await.0
    }

    async fn patch(&self, uri: &str, cookie: &str, body: Value) -> Value {
        self.call("PATCH", uri, Some(cookie), body).await.0
    }

    async fn put(&self, uri: &str, cookie: &str, body: Value) -> Value {
        self.call("PUT", uri, Some(cookie), body).await.0
    }
}

fn id(value: &Value) -> String {
    value["id"].as_str().expect("id").to_string()
}

/// A day `offset` days from today, as the API's day value.
fn day(offset: i64) -> String {
    let today = time::to_day(time::now());
    time::timestamp(time::add_seconds(today, offset * 86_400)).to_string()
}

fn paragraph(text: &str) -> Value {
    json!({ "type": "paragraph", "spans": [{ "text": text }] })
}

fn heading(text: &str) -> Value {
    json!({ "type": "heading", "attrs": { "level": 2 }, "spans": [{ "text": text }] })
}

fn bullet(text: &str) -> Value {
    json!({ "type": "bullet_item", "spans": [{ "text": text }] })
}

/// Fill the database behind `router`. Fails if it already has users.
pub async fn run(router: Router) -> Result<(), String> {
    let api = Api { router };
    let (status, _) = api.call("GET", "/auth/status", None, Value::Null).await;
    if status["bootstrap_required"] != true {
        return Err("the database already has users: run `mise run clean` to start over".into());
    }

    let (_, cookie) = api
        .call(
            "POST",
            "/auth/bootstrap",
            None,
            json!({ "email": "admin@woodmouse.test", "name": "Alex Admin", "password": PASSWORD }),
        )
        .await;
    let admin = cookie.expect("session cookie");

    let ws = id(&api
        .post(
            "/workspaces",
            &admin,
            json!({ "name": "Home", "description": "Household projects and notes" }),
        )
        .await);

    // A second person who can write, and a reader.
    let mut people = vec![];
    for (name, email, role) in [
        ("Sara Writer", "sara@woodmouse.test", "write"),
        ("Ola Reader", "ola@woodmouse.test", "read"),
    ] {
        let added = api
            .post(
                &format!("/workspaces/{ws}/members"),
                &admin,
                json!({ "name": name, "email": email, "role": role }),
            )
            .await;
        let token = added["invite_token"].as_str().expect("invite token");
        let (user, cookie) = api
            .call(
                "POST",
                &format!("/invites/{token}/accept"),
                None,
                json!({ "password": PASSWORD }),
            )
            .await;
        people.push((id(&user), cookie.expect("session")));
    }
    let sara = people[0].0.clone();
    let sara_cookie = people[0].1.clone();

    let mut labels = vec![];
    for (name, color) in [
        ("Urgent", "#dc2626"),
        ("Shopping", "#2563eb"),
        ("Outdoors", "#16a34a"),
    ] {
        labels.push(id(&api
            .post(
                &format!("/workspaces/{ws}/labels"),
                &admin,
                json!({ "name": name, "color": color }),
            )
            .await));
    }

    // House renovation: lists, dated tasks, subtasks, comments.
    let house = id(&api
        .post(
            &format!("/workspaces/{ws}/projects"),
            &admin,
            json!({ "name": "House renovation", "description": "Kitchen first, then the hallway" }),
        )
        .await);
    let mut lists = vec![];
    for name in ["To do", "Doing", "Waiting"] {
        lists.push(id(&api
            .post(
                &format!("/projects/{house}/lists"),
                &admin,
                json!({ "name": name }),
            )
            .await));
    }
    let oak = id(&api
        .post(
            &format!("/lists/{}/tasks", lists[1]),
            &admin,
            json!({ "title": "Order oak worktop", "description": "Check the **delivery date** first.", "priority": "high",
                    "start_date": day(-2), "due_date": day(3), "estimate_minutes": 480, "spent_minutes": 150, "assignee_ids": [sara], "label_ids": [labels[1]] }),
        )
        .await);
    for title in ["Measure the counter", "Compare three quotes"] {
        api.post(
            &format!("/tasks/{oak}/subtasks"),
            &admin,
            json!({ "title": title }),
        )
        .await;
    }
    api.post(
        &format!("/tasks/{oak}/comments"),
        &sara_cookie,
        json!({ "body": "Back in stock on the 17th." }),
    )
    .await;
    api.post(&format!("/lists/{}/tasks", lists[0]), &admin, json!({ "title": "Paint the hallway", "priority": "medium", "start_date": day(5), "due_date": day(9) })).await;
    api.post(&format!("/lists/{}/tasks", lists[0]), &admin, json!({ "title": "Book the electrician", "priority": "urgent", "due_date": day(1), "label_ids": [labels[0]] })).await;
    let done = id(&api
        .post(
            &format!("/lists/{}/tasks", lists[2]),
            &admin,
            json!({ "title": "Pick tiles", "due_date": day(-4) }),
        )
        .await);
    api.patch(
        &format!("/tasks/{done}"),
        &admin,
        json!({ "is_completed": true }),
    )
    .await;
    api.post(
        &format!("/projects/{house}/tasks"),
        &admin,
        json!({ "title": "Ask the neighbours about the skip" }),
    )
    .await;

    // Garden.
    let garden = id(&api
        .post(
            &format!("/workspaces/{ws}/projects"),
            &admin,
            json!({ "name": "Garden" }),
        )
        .await);
    let todo = id(&api
        .post(
            &format!("/projects/{garden}/lists"),
            &admin,
            json!({ "name": "Spring" }),
        )
        .await);
    for (title, due) in [
        ("Prune the roses", 6),
        ("Buy compost", 2),
        ("Fix the fence", 14),
    ] {
        api.post(
            &format!("/lists/{todo}/tasks"),
            &admin,
            json!({ "title": title, "due_date": day(due), "label_ids": [labels[2]] }),
        )
        .await;
    }

    // Wiki.
    let home = id(&api
        .post(
            &format!("/workspaces/{ws}/pages"),
            &admin,
            json!({ "title": "Household" }),
        )
        .await);
    api.put(
        &format!("/pages/{home}/blocks"),
        &admin,
        json!({ "blocks": [
        heading("Welcome"),
        paragraph("Everything we need to run the house, in one place."),
        bullet("Contacts and contractors"),
        bullet("Budget"),
    ] }),
    )
    .await;
    for (title, text) in [
        ("Contractors", "Electrician: call before 9."),
        ("Budget", "Kitchen: 12 000. Hallway: 3 500."),
    ] {
        let page = id(&api
            .post(
                &format!("/workspaces/{ws}/pages"),
                &admin,
                json!({ "title": title, "parent_id": home }),
            )
            .await);
        api.put(
            &format!("/pages/{page}/blocks"),
            &admin,
            json!({ "blocks": [heading(title), paragraph(text)] }),
        )
        .await;
    }

    println!("Seeded. Sign in with any of these (password: {PASSWORD}):");
    println!(
        "  admin@woodmouse.test (super admin)\n  sara@woodmouse.test (write)\n  ola@woodmouse.test (read)"
    );
    Ok(())
}
