//! Calendar links: secret feed URLs for tasks with dates, managed by their owners.

use api::db::prelude::*;
use axum::http::StatusCode;
use entity::users;
use entity::{sessions, workspace_members};
use serde_json::{Value, json};

use crate::common::*;

struct Fx {
    app: axum::Router,
    db: sea_orm::DatabaseConnection,
    cookie: String,
    ws: uuid::Uuid,
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

async fn body_json(res: axum::http::Response<axum::body::Body>) -> Value {
    let text = body_string(res.into_body()).await;
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

impl Fx {
    async fn call(&self, req: axum::http::Request<axum::body::Body>) -> (StatusCode, Value) {
        let res = request(&self.app, req).await;
        (res.status(), body_json(res).await)
    }
    async fn post(&self, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.call(post_json(uri, &body.to_string(), Some(cookie)))
            .await
    }
    async fn get(&self, cookie: &str, uri: &str) -> (StatusCode, Value) {
        self.call(get(uri, Some(cookie))).await
    }
    /// The feed at a link's path, fetched without logging in.
    async fn feed(&self, path: &str) -> (StatusCode, String) {
        let res = request(&self.app, get(path, None)).await;
        (res.status(), body_string(res.into_body()).await)
    }
    async fn member(&self, email: &str) -> String {
        let db = &self.db;
        let user = users::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            email: Set(email.to_string()),
            name: Set("".to_string()),
            password_hash: Set("x".to_string()),
            super_admin: Set(false),
            active: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        workspace_members::ActiveModel {
            user_id: Set(user.id),
            workspace_id: Set(self.ws),
            role: Set(entity::Role::Read),
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
}

const DAY: &str = "2026-10-16T00:00:00Z";
const DAY_PLUS_2: &str = "2026-10-18T00:00:00Z";

#[tokio::test]
async fn feeds_show_dated_tasks_and_revoking_stops_them() {
    let fx = fixture().await;
    let me = fx.get(&fx.cookie, "/api/auth/me").await.1["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, project) = fx
        .post(
            &fx.cookie,
            &format!("/api/workspaces/{}/projects", fx.ws),
            json!({ "name": "House" }),
        )
        .await;
    let project_id = project["id"].as_str().unwrap().to_string();
    let (_, created) = fx
        .post(
            &fx.cookie,
            &format!("/api/projects/{project_id}/lists"),
            json!({ "name": "Todo" }),
        )
        .await;
    let list = created["id"].as_str().unwrap().to_string();

    let task = |title: &str, dates: Value| {
        let (list, title) = (list.clone(), title.to_string());
        let (me, dates) = (me.clone(), dates.clone());
        let fx = &fx;
        async move {
            let (_, t) = fx
                .post(
                    &fx.cookie,
                    &format!("/api/lists/{list}/tasks"),
                    json!({ "title": title }),
                )
                .await;
            let id = t["id"].as_str().unwrap().to_string();
            let mut body = dates;
            body["assignee_ids"] = json!([me]);
            let res = request(
                &fx.app,
                patch_json(
                    &format!("/api/tasks/{id}"),
                    &body.to_string(),
                    Some(&fx.cookie),
                ),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
            id
        }
    };
    task(
        "Order oak",
        json!({ "start_date": DAY, "due_date": DAY_PLUS_2 }),
    )
    .await;
    let done = task("Pay deposit", json!({ "due_date": DAY })).await;
    task("Someday", json!({})).await;
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{done}"),
            r#"{"is_completed":true}"#,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let make = |scope: Value| fx.post(&fx.cookie, "/api/calendar-links", scope);
    let (status, mine) = make(json!({ "scope": "user", "name": "Phone" })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(mine["name"], "Phone");
    let (_, proj) = make(json!({ "scope": "project", "project_id": project_id })).await;
    assert_eq!(proj["name"], "House");
    let (_, wsl) = make(json!({ "scope": "workspace", "workspace_id": fx.ws })).await;
    assert_eq!(wsl["name"], "Home");
    assert_eq!(wsl["mode"], "read");

    // All three feeds hold the two dated tasks: a range, and a done one; not the undated one.
    for link in [&mine, &proj, &wsl] {
        let (status, text) = fx.feed(link["path"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(text.starts_with("BEGIN:VCALENDAR"));
        assert!(text.contains("SUMMARY:Order oak"), "{text}");
        assert!(text.contains("DTSTART;VALUE=DATE:20261016"));
        assert!(text.contains("DTEND;VALUE=DATE:20261019"));
        assert!(text.contains("SUMMARY:\u{2713} Pay deposit"));
        assert!(!text.contains("Someday"));
    }
    let res = request(&fx.app, get(mine["path"].as_str().unwrap(), None)).await;
    assert_eq!(
        res.headers()["content-type"],
        "text/calendar; charset=utf-8"
    );

    // The list shows them with when they were last used; filters narrow it.
    let (_, all) = fx.get(&fx.cookie, "/api/calendar-links").await;
    assert_eq!(all.as_array().unwrap().len(), 3);
    assert!(all[0]["last_used_at"].is_string());
    let (_, only_project) = fx
        .get(
            &fx.cookie,
            &format!("/api/calendar-links?project_id={project_id}"),
        )
        .await;
    assert_eq!(only_project.as_array().unwrap().len(), 1);

    // Revoking one stops its feed and nothing else.
    let id = mine["id"].as_str().unwrap();
    let res = request(
        &fx.app,
        delete(&format!("/api/calendar-links/{id}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fx.feed(mine["path"].as_str().unwrap()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.feed(proj["path"].as_str().unwrap()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        fx.feed("/api/calendar/not-a-secret.ics").await.0,
        StatusCode::NOT_FOUND
    );

    // Revoke all for a workspace, then everything.
    let res = request(
        &fx.app,
        delete(
            &format!("/api/calendar-links?workspace_id={}", fx.ws),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fx.feed(wsl["path"].as_str().unwrap()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(fx.get(&fx.cookie, "/api/calendar-links").await.1, json!([]));
}

#[tokio::test]
async fn links_belong_to_their_owner_and_follow_their_access() {
    let fx = fixture().await;
    let reader = fx.member("r@home.io").await;
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Private").await;

    // Any member can make a workspace link, but not for a workspace they are not in.
    let (status, link) = fx
        .post(
            &reader,
            "/api/calendar-links",
            json!({ "scope": "workspace", "workspace_id": fx.ws }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = fx
        .post(
            &reader,
            "/api/calendar-links",
            json!({ "scope": "workspace", "workspace_id": other_ws }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fx
        .post(
            &reader,
            "/api/calendar-links",
            json!({ "scope": "workspace" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nobody else sees or revokes it.
    assert_eq!(fx.get(&fx.cookie, "/api/calendar-links").await.1, json!([]));
    let id = link["id"].as_str().unwrap();
    let res = request(
        &fx.app,
        delete(&format!("/api/calendar-links/{id}"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // When the person loses access the feed stops, and it works again if they return.
    assert_eq!(
        fx.feed(link["path"].as_str().unwrap()).await.0,
        StatusCode::OK
    );
    let db = &fx.db;
    let user = users::Entity::find()
        .filter(users::Column::Email.eq("r@home.io"))
        .one_or_not_found(db)
        .await
        .unwrap();
    workspace_members::Entity::delete_many()
        .filter(workspace_members::Column::UserId.eq(user.id))
        .filter(workspace_members::Column::WorkspaceId.eq(fx.ws))
        .exec(db)
        .await
        .unwrap();
    assert_eq!(
        fx.feed(link["path"].as_str().unwrap()).await.0,
        StatusCode::NOT_FOUND
    );
    workspace_members::ActiveModel {
        user_id: Set(user.id),
        workspace_id: Set(fx.ws),
        role: Set(entity::Role::Read),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    assert_eq!(
        fx.feed(link["path"].as_str().unwrap()).await.0,
        StatusCode::OK
    );
}
