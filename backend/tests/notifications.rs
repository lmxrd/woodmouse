//! Notifications, watching, email (instant and digest), deadlines and the email settings.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};
use woodmouse::mail::Outbox;
use woodmouse::state::AppState;

use common::*;

struct Fx {
    app: axum::Router,
    state: AppState,
    outbox: Arc<Outbox>,
    admin: String,
    sara: String,
    sara_id: String,
    ws: uuid::Uuid,
    project: String,
}

async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).unwrap_or(Value::Null)
}

async fn fixture() -> Fx {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .unwrap();
    woodmouse::migrations::MIGRATIONS.apply(&db).await.unwrap();
    woodmouse::search::ensure_index(&mut db.clone())
        .await
        .unwrap_or_else(|_| panic!("index"));
    let outbox = Arc::new(Outbox::default());
    let state = AppState::new(db.clone()).with_mailer(outbox.clone());
    let app = woodmouse::routes::api(state.clone())
        .with_state(state.clone())
        .split_for_parts()
        .0;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    let sara = toasty::create!(woodmouse::models::User {
        email: "sara@home.io".to_string(),
        name: "Sara".to_string(),
        password_hash: "x".to_string(),
        super_admin: false,
        active: true,
    })
    .exec(&mut db.clone())
    .await
    .unwrap();
    toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: sara.id,
        workspace_id: ws,
        role: woodmouse::models::Role::Write
    })
    .exec(&mut db.clone())
    .await
    .unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    toasty::create!(woodmouse::models::Session {
        token: token.clone(),
        user_id: sara.id,
        expires_at: far_future()
    })
    .exec(&mut db.clone())
    .await
    .unwrap();

    let res = request(
        &app,
        post_json(
            &format!("/api/workspaces/{ws}/projects"),
            r#"{"name":"P"}"#,
            Some(&admin),
        ),
    )
    .await;
    let project = json_of(res).await["id"].as_str().unwrap().to_string();
    Fx {
        app,
        state,
        outbox,
        admin,
        sara: format!("woodmouse_session={token}"),
        sara_id: sara.id.to_string(),
        ws,
        project,
    }
}

impl Fx {
    async fn call(
        &self,
        method: &str,
        uri: &str,
        cookie: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let b = body.to_string();
        let req = match method {
            "POST" => post_json(uri, &b, Some(cookie)),
            "PATCH" => patch_json(uri, &b, Some(cookie)),
            "PUT" => put_json(uri, &b, Some(cookie)),
            _ => get(uri, Some(cookie)),
        };
        let res = request(&self.app, req).await;
        (res.status(), json_of(res).await)
    }
    async fn task(&self, extra: Value) -> String {
        let mut body = json!({"title": "Order oak"});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, t) = self
            .call(
                "POST",
                &format!("/api/projects/{}/tasks", self.project),
                &self.admin,
                body,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        t["id"].as_str().unwrap().to_string()
    }
    async fn inbox(&self, who: &str) -> Vec<Value> {
        self.call("GET", "/api/notifications", who, Value::Null)
            .await
            .1
            .as_array()
            .unwrap()
            .clone()
    }
    fn mails(&self) -> usize {
        self.outbox.sent.lock().unwrap().len()
    }
}

#[tokio::test]
async fn assignment_comments_and_reading() {
    let fx = fixture().await;
    let task = fx.task(json!({"assignee_ids": [fx.sara_id]})).await;

    let n = fx.inbox(&fx.sara).await;
    assert_eq!(n.len(), 1);
    assert_eq!(n[0]["kind"], "task_assigned");
    assert_eq!(n[0]["actor"]["email"], "admin@home.io");
    assert_eq!(n[0]["target_id"], task);
    assert!(
        fx.inbox(&fx.admin).await.is_empty(),
        "the actor is not told about their own doing"
    );

    // A comment by someone else reaches the person on the task.
    fx.call(
        "POST",
        &format!("/api/tasks/{task}/comments"),
        &fx.admin,
        json!({"body": "Back in stock on the 17th."}),
    )
    .await;
    let n = fx.inbox(&fx.sara).await;
    assert_eq!(n.len(), 2);
    assert_eq!(n[0]["kind"], "comment_added");
    assert_eq!(n[0]["data"]["preview"], "Back in stock on the 17th.");

    let count = fx
        .call(
            "GET",
            "/api/notifications/unread-count",
            &fx.sara,
            Value::Null,
        )
        .await
        .1;
    assert_eq!(count["count"], 2);
    let first = n[0]["id"].as_str().unwrap();
    let (status, _) = fx
        .call(
            "POST",
            &format!("/api/notifications/{first}/read"),
            &fx.sara,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Not yours: 404.
    let (status, _) = fx
        .call(
            "POST",
            &format!("/api/notifications/{first}/read"),
            &fx.admin,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        fx.call(
            "GET",
            "/api/notifications/unread-count",
            &fx.sara,
            Value::Null
        )
        .await
        .1["count"],
        1
    );
    fx.call("POST", "/api/notifications/read-all", &fx.sara, json!({}))
        .await;
    assert_eq!(
        fx.call(
            "GET",
            "/api/notifications/unread-count",
            &fx.sara,
            Value::Null
        )
        .await
        .1["count"],
        0
    );
}

#[tokio::test]
async fn watching_a_task_or_page_and_bursts_of_changes() {
    let fx = fixture().await;
    let task = fx.task(json!({})).await;
    assert!(fx.inbox(&fx.sara).await.is_empty());

    let (_, w) = fx
        .call(
            "GET",
            &format!("/api/tasks/{task}/watch"),
            &fx.sara,
            Value::Null,
        )
        .await;
    assert_eq!(w["watching"], false);
    fx.call(
        "PUT",
        &format!("/api/tasks/{task}/watch"),
        &fx.sara,
        json!({"watching": true}),
    )
    .await;
    assert_eq!(
        fx.call(
            "GET",
            &format!("/api/tasks/{task}/watch"),
            &fx.sara,
            Value::Null
        )
        .await
        .1["watching"],
        true
    );

    // Changes tell the watcher, and a burst while unread is one notification.
    fx.call(
        "PATCH",
        &format!("/api/tasks/{task}"),
        &fx.admin,
        json!({"priority": "high"}),
    )
    .await;
    fx.call(
        "PATCH",
        &format!("/api/tasks/{task}"),
        &fx.admin,
        json!({"is_completed": true}),
    )
    .await;
    let n = fx.inbox(&fx.sara).await;
    assert_eq!(n.len(), 1);
    assert_eq!(n[0]["kind"], "task_changed");
    assert_eq!(n[0]["data"]["what"], "completed");

    // Stopping stops it.
    fx.call(
        "PUT",
        &format!("/api/tasks/{task}/watch"),
        &fx.sara,
        json!({"watching": false}),
    )
    .await;
    fx.call("POST", "/api/notifications/read-all", &fx.sara, json!({}))
        .await;
    fx.call(
        "PATCH",
        &format!("/api/tasks/{task}"),
        &fx.admin,
        json!({"is_completed": false}),
    )
    .await;
    assert_eq!(fx.inbox(&fx.sara).await.len(), 1);

    // Pages: watchers hear about edits.
    let (_, page) = fx
        .call(
            "POST",
            &format!("/api/workspaces/{}/pages", fx.ws),
            &fx.admin,
            json!({"title": "Plans"}),
        )
        .await;
    let page = page["id"].as_str().unwrap().to_string();
    fx.call(
        "PUT",
        &format!("/api/pages/{page}/watch"),
        &fx.sara,
        json!({"watching": true}),
    )
    .await;
    fx.call(
        "PUT",
        &format!("/api/pages/{page}/blocks"),
        &fx.admin,
        json!({"blocks": [{"type": "paragraph", "spans": [{"text": "hello"}]}]}),
    )
    .await;
    let n = fx.inbox(&fx.sara).await;
    assert_eq!(n[0]["kind"], "page_changed");
    assert_eq!(n[0]["data"]["title"], "Plans");
}

#[tokio::test]
async fn mentioning_a_task_tells_the_people_on_it() {
    let fx = fixture().await;
    let task = fx.task(json!({"assignee_ids": [fx.sara_id]})).await;
    fx.call("POST", "/api/notifications/read-all", &fx.sara, json!({}))
        .await;
    let (_, page) = fx
        .call(
            "POST",
            &format!("/api/workspaces/{}/pages", fx.ws),
            &fx.admin,
            json!({"title": "Notes"}),
        )
        .await;
    let page = page["id"].as_str().unwrap().to_string();
    let blocks = json!({"blocks": [{"type": "paragraph", "spans": [{"mention": {"task_id": task, "label": "Order oak"}}]}]});
    fx.call(
        "PUT",
        &format!("/api/pages/{page}/blocks"),
        &fx.admin,
        blocks.clone(),
    )
    .await;
    let n = fx.inbox(&fx.sara).await;
    assert_eq!(n[0]["kind"], "mentioned");
    assert_eq!(n[0]["target_id"], page);
    // Saving the same mention again is not news.
    fx.call("POST", "/api/notifications/read-all", &fx.sara, json!({}))
        .await;
    fx.call(
        "PUT",
        &format!("/api/pages/{page}/blocks"),
        &fx.admin,
        blocks,
    )
    .await;
    assert!(fx.inbox(&fx.sara).await.iter().all(|n| n["read"] == true));
}

#[tokio::test]
async fn instant_email_follows_each_persons_setting() {
    let fx = fixture().await;
    let now = woodmouse::time::now();
    fx.task(json!({"assignee_ids": [fx.sara_id]})).await;

    // Not wanted: nothing is sent, and it is not looked at again.
    woodmouse::jobs::send_pending_emails(&fx.state, now).await;
    assert_eq!(fx.mails(), 0);

    let (status, s) = fx
        .call(
            "PATCH",
            "/api/notification-settings",
            &fx.sara,
            json!({"email_instant": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s["email_instant"], true);
    fx.task(json!({"title": "Second", "assignee_ids": [fx.sara_id]}))
        .await;
    woodmouse::jobs::send_pending_emails(&fx.state, now).await;
    assert_eq!(fx.mails(), 1);
    let mail = fx.outbox.sent.lock().unwrap()[0].clone();
    assert_eq!(mail.to, "sara@home.io");
    assert!(mail.subject.contains("assigned you"), "{}", mail.subject);
    // Once only.
    woodmouse::jobs::send_pending_emails(&fx.state, now).await;
    assert_eq!(fx.mails(), 1);

    let (status, _) = fx
        .call(
            "PATCH",
            "/api/notification-settings",
            &fx.sara,
            json!({"digest_hour": 30}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn deadlines_are_notified_once_and_digests_go_out_at_the_hour() {
    let fx = fixture().await;
    let now = woodmouse::time::now();
    let tomorrow = woodmouse::time::timestamp(woodmouse::time::add_seconds(
        woodmouse::time::to_day(now),
        86_400,
    ))
    .to_string();
    let yesterday = woodmouse::time::timestamp(woodmouse::time::add_seconds(
        woodmouse::time::to_day(now),
        -86_400,
    ))
    .to_string();
    fx.task(json!({"title": "Soon", "assignee_ids": [fx.sara_id], "due_date": tomorrow}))
        .await;
    fx.task(json!({"title": "Late", "assignee_ids": [fx.sara_id], "due_date": yesterday}))
        .await;
    fx.call("POST", "/api/notifications/read-all", &fx.sara, json!({}))
        .await;

    woodmouse::jobs::deadline_notifications(&fx.state, now).await;
    woodmouse::jobs::deadline_notifications(&fx.state, now).await;
    let n = fx.inbox(&fx.sara).await;
    let unread: Vec<&Value> = n.iter().filter(|n| n["read"] == false).collect();
    assert_eq!(unread.len(), 2, "once per task: {n:?}");
    let kinds: Vec<&str> = unread.iter().map(|n| n["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"due_soon") && kinds.contains(&"overdue"));

    // The digest goes out in the person's chosen hour (UTC while they have no timezone), once a day.
    let hour = i64::from(now.hour());
    fx.call(
        "PATCH",
        "/api/notification-settings",
        &fx.sara,
        json!({"digest": true, "digest_hour": hour}),
    )
    .await;
    woodmouse::jobs::send_digests(&fx.state, now).await;
    assert_eq!(fx.mails(), 1);
    let mail = fx.outbox.sent.lock().unwrap()[0].clone();
    assert_eq!(mail.subject, "Your Woodmouse summary");
    assert!(
        mail.text.contains("Soon") || mail.text.contains("is due tomorrow"),
        "{}",
        mail.text
    );
    woodmouse::jobs::send_digests(&fx.state, now).await;
    assert_eq!(fx.mails(), 1);
    // Another hour: nothing.
    let other = (hour + 5) % 24;
    fx.call(
        "PATCH",
        "/api/notification-settings",
        &fx.sara,
        json!({"digest_hour": other}),
    )
    .await;
    woodmouse::jobs::send_digests(&fx.state, woodmouse::time::add_seconds(now, 25 * 3600)).await;
    assert_eq!(fx.mails(), 1);
}

#[tokio::test]
async fn the_super_admin_edits_email_settings_and_sends_a_test() {
    let fx = fixture().await;
    let (status, _) = fx
        .call("GET", "/api/admin/email", &fx.sara, Value::Null)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, v) = fx
        .call(
            "PUT",
            "/api/admin/email",
            &fx.admin,
            json!({"host": "smtp.example.com", "port": 587, "from_address": "woodmouse@example.com", "password": "s3cret", "tls": "starttls"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["host"], "smtp.example.com");
    assert_eq!(v["has_password"], true);
    assert!(
        v.get("password").is_none(),
        "the password is never sent back"
    );

    // Leaving the password out keeps it; an invalid value is refused.
    let (_, v) = fx
        .call(
            "PUT",
            "/api/admin/email",
            &fx.admin,
            json!({"host": "smtp.example.org"}),
        )
        .await;
    assert_eq!(v["host"], "smtp.example.org");
    assert_eq!(v["has_password"], true);
    let (status, _) = fx
        .call(
            "PUT",
            "/api/admin/email",
            &fx.admin,
            json!({"tls": "carrier-pigeon"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, r) = fx
        .call("POST", "/api/admin/email/test", &fx.admin, json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(r["sent_to"], "admin@home.io");
    assert_eq!(fx.mails(), 1);
}
