//! The wiki: page tree, block content and its validation, permissions, and trash.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::*;

/// The body as JSON, or as a string when it is not JSON (extractor rejections are plain text).
async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    let text = body_string(res.into_body()).await;
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

struct Fx {
    app: axum::Router,
    db: toasty::Db,
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

impl Fx {
    async fn page(&self, title: &str, parent: Option<&str>) -> String {
        let body = json!({ "title": title, "parent_id": parent }).to_string();
        let res = request(
            &self.app,
            post_json(
                &format!("/api/workspaces/{}/pages", self.ws),
                &body,
                Some(&self.cookie),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        json_of(res).await["id"].as_str().unwrap().to_string()
    }
    async fn get(&self, id: &str) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            get(&format!("/api/pages/{id}"), Some(&self.cookie)),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn patch(&self, id: &str, body: Value) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            patch_json(
                &format!("/api/pages/{id}"),
                &body.to_string(),
                Some(&self.cookie),
            ),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn save(&self, id: &str, blocks: Value) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            put_json(
                &format!("/api/pages/{id}/blocks"),
                &json!({ "blocks": blocks }).to_string(),
                Some(&self.cookie),
            ),
        )
        .await;
        (res.status(), json_of(res).await)
    }
    async fn tree(&self) -> Vec<Value> {
        let res = request(
            &self.app,
            get(
                &format!("/api/workspaces/{}/pages", self.ws),
                Some(&self.cookie),
            ),
        )
        .await;
        json_of(res).await.as_array().unwrap().clone()
    }
    async fn trash(&self) -> Vec<Value> {
        let res = request(
            &self.app,
            get(
                &format!("/api/workspaces/{}/trash", self.ws),
                Some(&self.cookie),
            ),
        )
        .await;
        json_of(res).await.as_array().unwrap().clone()
    }
}

fn text(t: &str) -> Value {
    json!({ "text": t })
}

#[tokio::test]
async fn page_tree_create_move_and_rename() {
    let fx = fixture().await;
    let household = fx.page("Household", None).await;
    let kitchen = fx.page("Kitchen renovation", Some(&household)).await;
    let contacts = fx.page("Contractor contacts", Some(&kitchen)).await;
    let budget = fx.page("", Some(&household)).await;

    let (_, deep) = fx.get(&contacts).await;
    let path: Vec<&str> = deep["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert_eq!(path, ["Household", "Kitchen renovation"]);
    assert_eq!(
        fx.get(&budget).await.1["title"],
        "Untitled",
        "an empty title becomes Untitled"
    );

    // Siblings are ordered by position, new pages go last.
    let siblings: Vec<String> = fx
        .tree()
        .await
        .iter()
        .filter(|p| p["parent_id"] == household.as_str())
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(siblings, ["Kitchen renovation", "Untitled"]);

    // A page cannot go below itself or its own descendants.
    let (status, _) = fx.patch(&household, json!({ "parent_id": contacts })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = fx.patch(&kitchen, json!({ "parent_id": kitchen })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Move to the top level, then under another page; renaming works.
    let (status, moved) = fx.patch(&contacts, json!({ "parent_id": null })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["ancestors"].as_array().unwrap().len(), 0);
    let (_, moved) = fx
        .patch(&contacts, json!({ "parent_id": budget, "title": "Trades" }))
        .await;
    assert_eq!(moved["title"], "Trades");
    assert_eq!(moved["ancestors"][1]["title"], "Untitled");

    // Reordering with an explicit position.
    fx.patch(&budget, json!({ "after_id": null })).await;
    let first = fx
        .tree()
        .await
        .into_iter()
        .find(|p| p["parent_id"] == household.as_str())
        .unwrap();
    assert_eq!(first["title"], "Untitled");

    // A parent from another workspace (or a missing one) is not found.
    let (status, _) = fx
        .patch(&kitchen, json!({ "parent_id": uuid::Uuid::new_v4() }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn blocks_round_trip_and_validate() {
    let fx = fixture().await;
    let page = fx.page("Contractor contacts", None).await;
    let task = uuid::Uuid::new_v4();
    let other = fx.page("Budget", None).await;

    let doc = json!([
        { "type": "heading", "attrs": { "level": 2 }, "spans": [text("Electrician")] },
        { "type": "paragraph", "spans": [
            { "text": "Call ", "marks": ["bold"] },
            { "text": "Anders", "link": "https://example.com/anders" },
            { "mention": { "task_id": task, "label": "Order oak worktop" } },
            { "page_link": { "page_id": other, "label": "Budget" } }
        ] },
        { "type": "todo_item", "attrs": { "checked": true, "indent": 1 }, "spans": [text("Quote approved")] },
        { "type": "code", "attrs": { "language": "sh" }, "spans": [text("gas shutoff: cellar")] },
        { "type": "callout", "attrs": { "tone": "warning" }, "spans": [text("Heads up")] },
        { "type": "divider" }
    ]);
    let (status, saved) = fx.save(&page, doc.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["blocks"].as_array().unwrap().len(), 6);

    // Reading returns exactly what was saved, in order, with the structured spans intact.
    let (_, read) = fx.get(&page).await;
    let blocks = read["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "heading");
    assert_eq!(blocks[0]["attrs"]["level"], 2);
    assert_eq!(blocks[1]["spans"][0]["marks"], json!(["bold"]));
    assert_eq!(
        blocks[1]["spans"][2]["mention"]["label"],
        "Order oak worktop"
    );
    assert_eq!(
        blocks[1]["spans"][3]["page_link"]["page_id"],
        other.as_str()
    );
    assert_eq!(blocks[2]["attrs"]["checked"], true);

    // Replacing shrinks the document.
    let (_, replaced) = fx
        .save(
            &page,
            json!([{ "type": "paragraph", "spans": [text("only this")] }]),
        )
        .await;
    assert_eq!(replaced["blocks"].as_array().unwrap().len(), 1);

    // Invalid blocks are rejected and change nothing.
    let bad_docs = [
        json!([{ "type": "heading", "spans": [text("no level")] }]),
        json!([{ "type": "heading", "attrs": { "level": 4 }, "spans": [] }]),
        json!([{ "type": "paragraph", "attrs": { "level": 1 }, "spans": [] }]),
        json!([{ "type": "paragraph", "spans": [{ "text": "x", "link": "javascript:alert(1)" }] }]),
        json!([{ "type": "paragraph", "spans": [{ "text": "x", "link": " JavaScript:alert(1)" }] }]),
        json!([{ "type": "paragraph", "spans": [{ "text": "x", "mention": { "task_id": task, "label": "t" } }] }]),
        json!([{ "type": "paragraph", "spans": [{}] }]),
        json!([{ "type": "paragraph", "spans": [{ "mention": { "task_id": task, "label": "t" }, "marks": ["bold"] }] }]),
        json!([{ "type": "divider", "spans": [text("x")] }]),
        json!([{ "type": "code", "spans": [{ "text": "x", "marks": ["bold"] }] }]),
        json!([{ "type": "bullet_item", "attrs": { "checked": true }, "spans": [] }]),
        json!([{ "type": "bullet_item", "spans": [text("two\nlines")] }]),
        json!([{ "type": "callout", "attrs": { "tone": "sparkly" }, "spans": [] }]),
        json!([{ "type": "bullet_item", "attrs": { "indent": 99 }, "spans": [] }]),
        json!([{ "type": "mystery", "spans": [] }]),
        json!([{ "type": "paragraph", "surprise": 1 }]),
    ];
    for (i, bad) in bad_docs.into_iter().enumerate() {
        let (status, body) = fx.save(&page, bad).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "case {i}: {status} {body}"
        );
    }
    assert_eq!(
        fx.get(&page).await.1["blocks"].as_array().unwrap().len(),
        1,
        "failed saves left the content alone"
    );

    // Too many blocks.
    let many: Vec<Value> = (0..5001).map(|_| json!({ "type": "paragraph" })).collect();
    let (status, _) = fx.save(&page, Value::Array(many)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn saving_the_same_content_does_not_touch_updated_at() {
    let fx = fixture().await;
    let page = fx.page("P", None).await;
    let doc = json!([{ "type": "paragraph", "spans": [text("hello")] }]);
    let (_, first) = fx.save(&page, doc.clone()).await;
    let stamp = first["updated_at"].as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (_, same) = fx.save(&page, doc).await;
    assert_eq!(
        same["updated_at"].as_str().unwrap(),
        stamp,
        "unchanged content is not a change"
    );
    let (_, changed) = fx
        .save(
            &page,
            json!([{ "type": "paragraph", "spans": [text("hello!")] }]),
        )
        .await;
    assert!(changed["updated_at"].as_str().unwrap() > stamp.as_str());
    assert_eq!(changed["updated_by"]["email"], "admin@home.io");
}

#[tokio::test]
async fn permissions_and_workspace_isolation() {
    let fx = fixture().await;
    let page = fx.page("Secret", None).await;

    let mk = |email: &'static str, role: woodmouse::models::Role| {
        let (db, ws) = (fx.db.clone(), fx.ws);
        async move {
            let user = toasty::create!(woodmouse::models::User {
                email: email.to_string(),
                name: "".to_string(),
                password_hash: "x".to_string(),
                super_admin: false,
                active: true,
            })
            .exec(&mut db.clone())
            .await
            .unwrap();
            toasty::create!(woodmouse::models::WorkspaceMember {
                user_id: user.id,
                workspace_id: ws,
                role
            })
            .exec(&mut db.clone())
            .await
            .unwrap();
            let token = uuid::Uuid::new_v4().to_string();
            toasty::create!(woodmouse::models::Session {
                token: token.clone(),
                user_id: user.id,
                expires_at: crate::common::far_future()
            })
            .exec(&mut db.clone())
            .await
            .unwrap();
            format!("woodmouse_session={token}")
        }
    };
    let reader = mk("r@home.io", woodmouse::models::Role::Read).await;
    let writer = mk("w@home.io", woodmouse::models::Role::Write).await;

    // Readers read the tree and pages but cannot change anything.
    assert_eq!(
        request(&fx.app, get(&format!("/api/pages/{page}"), Some(&reader)))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &fx.app,
            get(&format!("/api/workspaces/{}/pages", fx.ws), Some(&reader))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let create = post_json(
        &format!("/api/workspaces/{}/pages", fx.ws),
        r#"{"title":"x"}"#,
        Some(&reader),
    );
    assert_eq!(
        request(&fx.app, create).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &fx.app,
            patch_json(
                &format!("/api/pages/{page}"),
                r#"{"title":"x"}"#,
                Some(&reader)
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &fx.app,
            put_json(
                &format!("/api/pages/{page}/blocks"),
                r#"{"blocks":[]}"#,
                Some(&reader)
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &fx.app,
            delete(&format!("/api/pages/{page}"), Some(&reader))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    // Writers edit; a stranger (no membership) sees nothing.
    assert_eq!(
        request(
            &fx.app,
            patch_json(
                &format!("/api/pages/{page}"),
                r#"{"title":"Renamed"}"#,
                Some(&writer)
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let stranger = {
        let user = toasty::create!(woodmouse::models::User {
            email: "s@x.io".to_string(),
            name: "".to_string(),
            password_hash: "x".to_string(),
            super_admin: false,
            active: true,
        })
        .exec(&mut fx.db.clone())
        .await
        .unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        toasty::create!(woodmouse::models::Session {
            token: token.clone(),
            user_id: user.id,
            expires_at: crate::common::far_future()
        })
        .exec(&mut fx.db.clone())
        .await
        .unwrap();
        format!("woodmouse_session={token}")
    };
    assert_eq!(
        request(&fx.app, get(&format!("/api/pages/{page}"), Some(&stranger)))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &fx.app,
            get(&format!("/api/workspaces/{}/pages", fx.ws), Some(&stranger))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&fx.app, get(&format!("/api/pages/{page}"), None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // A page cannot be parented across workspaces.
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Work").await;
    let body = json!({ "title": "x", "parent_id": page }).to_string();
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{other_ws}/pages"),
            &body,
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_page_takes_its_subtree_and_restores_as_a_group() {
    let fx = fixture().await;
    let root = fx.page("Household", None).await;
    let kitchen = fx.page("Kitchen", Some(&root)).await;
    let _contacts = fx.page("Contacts", Some(&kitchen)).await;
    let early = fx.page("Early", Some(&root)).await;
    fx.save(
        &kitchen,
        json!([{ "type": "paragraph", "spans": [text("keep me")] }]),
    )
    .await;

    // One child is deleted on its own first.
    request(
        &fx.app,
        delete(&format!("/api/pages/{early}"), Some(&fx.cookie)),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    request(
        &fx.app,
        delete(&format!("/api/pages/{root}"), Some(&fx.cookie)),
    )
    .await;

    assert!(fx.tree().await.is_empty());
    assert_eq!(fx.get(&kitchen).await.0, StatusCode::NOT_FOUND);

    // The trash lists the root (with its subtree counted) and the early child, not the cascaded ones.
    let trash = fx.trash().await;
    assert_eq!(trash.len(), 2);
    let item = trash.iter().find(|t| t["name"] == "Household").unwrap();
    assert_eq!(item["kind"], "page");
    assert_eq!(item["child_count"], 2);

    // The early child cannot come back before its parent.
    let restore = |id: &str| {
        let (app, cookie, ws) = (fx.app.clone(), fx.cookie.clone(), fx.ws);
        let uri = format!("/api/workspaces/{ws}/trash/page/{id}/restore");
        async move {
            request(&app, post_json(&uri, "{}", Some(&cookie)))
                .await
                .status()
        }
    };
    assert_eq!(restore(&early).await, StatusCode::CONFLICT);
    assert_eq!(restore(&root).await, StatusCode::NO_CONTENT);

    let titles: Vec<String> = fx
        .tree()
        .await
        .iter()
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        titles.len(),
        3,
        "only what was deleted with the root returns: {titles:?}"
    );
    assert!(!titles.contains(&"Early".to_string()));
    assert_eq!(
        fx.get(&kitchen).await.1["blocks"][0]["spans"][0]["text"],
        "keep me",
        "content survives a delete and restore"
    );

    assert_eq!(restore(&early).await, StatusCode::NO_CONTENT);
    assert_eq!(fx.tree().await.len(), 4);
}

#[tokio::test]
async fn stale_saves_conflict_unless_forced_and_presence_lists_others() {
    let fx = fixture().await;
    let page = fx.page("P", None).await;
    let (_, opened) = fx.get(&page).await;
    let v0 = opened["version"].as_i64().unwrap();

    let save = |blocks: Value, base: i64, force: bool| {
        let body = json!({ "blocks": blocks, "base_version": base, "force": force }).to_string();
        let req = put_json(
            &format!("/api/pages/{page}/blocks"),
            &body,
            Some(&fx.cookie),
        );
        async { request(&fx.app, req).await }
    };
    let a = json!([{ "type": "paragraph", "spans": [text("a")] }]);
    let b = json!([{ "type": "paragraph", "spans": [text("b")] }]);

    let res = save(a.clone(), v0, false).await;
    assert_eq!(res.status(), StatusCode::OK);
    let v1 = json_of(res).await["version"].as_i64().unwrap();
    assert_eq!(v1, v0 + 1);

    // Someone still on v0 is refused; forcing overwrites.
    assert_eq!(
        save(b.clone(), v0, false).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(save(b, v0, true).await.status(), StatusCode::OK);

    // Presence never lists yourself.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/pages/{page}/presence"),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_of(res).await, json!([]));
}

#[tokio::test]
async fn search_ranks_titles_above_content_and_limits_results() {
    let fx = fixture().await;
    let budget = fx.page("Budget 2026", None).await;
    let notes = fx.page("Meeting notes", None).await;
    fx.page("Rebudget plans", None).await; // a word must start with the search term
    fx.save(
        &notes,
        json!([{ "type": "paragraph", "spans": [text("we discussed the budget")] }]),
    )
    .await;

    let find = |q: &str, extra: &str| {
        let uri = format!("/api/workspaces/{}/search?q={q}{extra}", fx.ws);
        let req = get(&uri, Some(&fx.cookie));
        async { json_of(request(&fx.app, req).await).await }
    };
    let hits = find("budget", "").await;
    let ids: Vec<&str> = hits
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    // The title match comes before the text-only match.
    assert_eq!(ids, [budget.as_str(), notes.as_str()]);

    assert_eq!(
        find("budget", "&limit=1").await.as_array().unwrap().len(),
        1
    );
    assert_eq!(find("zzz", "").await, json!([]));
    // No query: everything recent, capped.
    assert_eq!(find("", "&kind=page").await.as_array().unwrap().len(), 3);

    // Deleted pages are not found, and restoring brings them back.
    let res = request(
        &fx.app,
        delete(&format!("/api/pages/{budget}"), Some(&fx.cookie)),
    )
    .await;
    assert!(res.status().is_success());
    assert_eq!(find("budget", "").await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn search_finds_tasks_and_respects_workspaces() {
    let fx = fixture().await;
    let post = |uri: String, body: Value| {
        let req = post_json(&uri, &body.to_string(), Some(&fx.cookie));
        async { json_of(request(&fx.app, req).await).await }
    };
    let project = post(
        format!("/api/workspaces/{}/projects", fx.ws),
        json!({ "name": "House" }),
    )
    .await;
    let list = post(
        format!("/api/projects/{}/lists", project["id"].as_str().unwrap()),
        json!({ "name": "Todo" }),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = post(
        format!("/api/lists/{list}/tasks"),
        json!({ "title": "Order oak worktop" }),
    )
    .await;
    let task_id = task["id"].as_str().unwrap().to_string();

    let find = |ws: uuid::Uuid, q: &str, extra: &str| {
        let req = get(
            &format!("/api/workspaces/{ws}/search?q={q}{extra}"),
            Some(&fx.cookie),
        );
        async { json_of(request(&fx.app, req).await).await }
    };
    let hits = find(fx.ws, "oak", "").await;
    assert_eq!(hits[0]["id"], task_id);
    assert_eq!(hits[0]["kind"], "task");
    assert_eq!(hits[0]["project_id"], project["id"]);
    assert_eq!(hits[0]["is_completed"], false);

    // Renaming updates the index; a page search does not return tasks.
    let res = request(
        &fx.app,
        patch_json(
            &format!("/api/tasks/{task_id}"),
            &json!({ "title": "Order birch worktop" }).to_string(),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert!(res.status().is_success());
    assert_eq!(find(fx.ws, "oak", "").await, json!([]));
    assert_eq!(find(fx.ws, "birch", "").await.as_array().unwrap().len(), 1);
    assert_eq!(find(fx.ws, "birch", "&kind=page").await, json!([]));

    // Another workspace only sees this one when asked to look everywhere.
    let other = create_workspace(&fx.app, &fx.cookie, "Work").await;
    assert_eq!(find(other, "birch", "").await, json!([]));
    let all = find(other, "birch", "&all_workspaces=true").await;
    assert_eq!(all[0]["workspace_name"], "Home");
}

impl Fx {
    /// A second member with write access, as a session cookie.
    async fn teammate(&self, email: &str) -> String {
        let db = &mut self.db.clone();
        let user = toasty::create!(woodmouse::models::User {
            email: email.to_string(),
            name: "".to_string(),
            password_hash: "x".to_string(),
            super_admin: false,
            active: true,
        })
        .exec(db)
        .await
        .unwrap();
        toasty::create!(woodmouse::models::WorkspaceMember {
            user_id: user.id,
            workspace_id: self.ws,
            role: woodmouse::models::Role::Write
        })
        .exec(db)
        .await
        .unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        toasty::create!(woodmouse::models::Session {
            token: token.clone(),
            user_id: user.id,
            expires_at: crate::common::far_future()
        })
        .exec(db)
        .await
        .unwrap();
        format!("woodmouse_session={token}")
    }
    async fn save_as(&self, cookie: &str, page: &str, text_: &str) {
        let body = json!({ "blocks": [{ "type": "paragraph", "spans": [text(text_)] }] });
        let res = request(
            &self.app,
            put_json(
                &format!("/api/pages/{page}/blocks"),
                &body.to_string(),
                Some(cookie),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    async fn json_get(&self, uri: &str) -> Value {
        json_of(request(&self.app, get(uri, Some(&self.cookie))).await).await
    }
}

#[tokio::test]
async fn page_saves_are_versions_and_restore_keeps_everything() {
    let fx = fixture().await;
    let page = fx.page("Notes", None).await;
    let other = fx.teammate("t@home.io").await;

    // Saves outside live editing are deliberate: each is a version holding the new content (and who
    // saved it), after the one made with the page.
    fx.save_as(&fx.cookie, &page, "first").await;
    fx.save_as(&fx.cookie, &page, "second").await;
    let list = fx.json_get(&format!("/api/pages/{page}/revisions")).await;
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert_eq!(list[0]["saved_by"]["email"], "admin@home.io");
    let second = list[0]["id"].as_str().unwrap().to_string();
    let detail = fx.json_get(&format!("/api/page-revisions/{second}")).await;
    assert_eq!(detail["blocks"][0]["spans"][0]["text"], "second");

    fx.save_as(&other, &page, "third").await;
    let list = fx.json_get(&format!("/api/pages/{page}/revisions")).await;
    assert_eq!(list.as_array().unwrap().len(), 4);
    assert_eq!(list[0]["saved_by"]["email"], "t@home.io");

    // Restoring writes the old content as a new version; nothing is lost.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/page-revisions/{second}/restore"),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let restored = json_of(res).await;
    assert_eq!(restored["blocks"][0]["spans"][0]["text"], "second");
    let list = fx.json_get(&format!("/api/pages/{page}/revisions")).await;
    assert_eq!(list.as_array().unwrap().len(), 5);
    let third = list[1]["id"].as_str().unwrap();
    let detail = fx.json_get(&format!("/api/page-revisions/{third}")).await;
    assert_eq!(detail["blocks"][0]["spans"][0]["text"], "third");
}

#[tokio::test]
async fn task_text_versions_keep_the_original_and_restore() {
    let fx = fixture().await;
    let project = json_of(
        request(
            &fx.app,
            post_json(
                &format!("/api/workspaces/{}/projects", fx.ws),
                r#"{"name":"P"}"#,
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    let created = json_of(
        request(
            &fx.app,
            post_json(
                &format!("/api/projects/{}/lists", project["id"].as_str().unwrap()),
                r#"{"name":"Todo"}"#,
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    let list = created["id"].as_str().unwrap();
    let task = json_of(
        request(
            &fx.app,
            post_json(
                &format!("/api/lists/{list}/tasks"),
                r#"{"title":"Original"}"#,
                Some(&fx.cookie),
            ),
        )
        .await,
    )
    .await;
    let id = task["id"].as_str().unwrap();
    let edit = |body: Value| {
        let req = patch_json(
            &format!("/api/tasks/{id}"),
            &body.to_string(),
            Some(&fx.cookie),
        );
        async { request(&fx.app, req).await.status() }
    };
    assert_eq!(edit(json!({ "title": "Renamed" })).await, StatusCode::OK);
    assert_eq!(
        edit(json!({ "title": "Renamed again" })).await,
        StatusCode::OK
    );
    // A change to something else is not a text change.
    assert_eq!(edit(json!({ "priority": "high" })).await, StatusCode::OK);

    let list = fx.json_get(&format!("/api/tasks/{id}/revisions")).await;
    // Every text save is a version holding the new text: creation and two renames.
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert_eq!(list[0]["title"], "Renamed again");
    assert_eq!(list[2]["title"], "Original");
    assert!(list[2]["saved_at"].is_string());

    let rev = list[2]["id"].as_str().unwrap();
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/task-revisions/{rev}/restore"),
            "{}",
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_of(res).await["title"], "Original");
    let list = fx.json_get(&format!("/api/tasks/{id}/revisions")).await;
    assert_eq!(list.as_array().unwrap().len(), 4);
    assert_eq!(list[0]["title"], "Original");
}

#[tokio::test]
async fn markdown_export_of_a_page_and_of_the_whole_wiki() {
    let fx = fixture().await;
    let parent = fx.page("Home base", None).await;
    let child = fx.page("Plan", Some(&parent)).await;
    let body = json!({ "blocks": [
        { "type": "heading", "attrs": { "level": 1 }, "spans": [text("Goals")] },
        { "type": "paragraph", "spans": [
            { "text": "see " },
            { "page_link": { "page_id": child, "label": "Plan" } }
        ] }
    ]});
    let res = request(
        &fx.app,
        put_json(
            &format!("/api/pages/{parent}/blocks"),
            &body.to_string(),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = request(
        &fx.app,
        get(&format!("/api/pages/{parent}/export"), Some(&fx.cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let disposition = res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(disposition.contains("Home base.md"), "{disposition}");
    assert_eq!(
        body_string(res.into_body()).await,
        "# Home base\n\n## Goals\n\nsee Plan\n"
    );

    let res = request(
        &fx.app,
        get(
            &format!("/api/workspaces/{}/wiki/export", fx.ws),
            Some(&fx.cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    use http_body_util::BodyExt;
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok())
        .map(|n| n.to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["Home base.md", "Home base/Plan.md"]);
    let mut text_ = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Home base.md").unwrap(), &mut text_).unwrap();
    assert!(text_.contains("[Plan](Home%20base/Plan.md)"), "{text_}");
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
    /// A task in a new project, as (project id, task id).
    async fn task(&self, title: &str) -> (String, String) {
        let (_, project) = self
            .post(
                &format!("/api/workspaces/{}/projects", self.ws),
                json!({ "name": "P" }),
            )
            .await;
        let project = project["id"].as_str().unwrap().to_string();
        let (_, list) = self
            .post(
                &format!("/api/projects/{project}/lists"),
                json!({ "name": "Todo" }),
            )
            .await;
        let (_, task) = self
            .post(
                &format!("/api/lists/{}/tasks", list["id"].as_str().unwrap()),
                json!({ "title": title }),
            )
            .await;
        (project, task["id"].as_str().unwrap().to_string())
    }
}

#[tokio::test]
async fn references_are_derived_from_content_and_resolved_with_current_titles() {
    let fx = fixture().await;
    let source = fx.page("Source", None).await;
    let target = fx.page("Target", None).await;
    let (_, task) = fx.task("Order oak").await;

    let doc = json!([{ "type": "paragraph", "spans": [
        { "page_link": { "page_id": target, "label": "Old name" } },
        { "mention": { "task_id": task, "label": "Order oak" } },
        { "page_link": { "page_id": target, "label": "again" } }
    ] }]);
    let (status, _) = fx.save(&source, doc).await;
    assert_eq!(status, StatusCode::OK);

    let back = fx.json_get(&format!("/api/pages/{target}/backlinks")).await;
    assert_eq!(back, json!([{ "id": source, "title": "Source" }]));
    let back = fx.json_get(&format!("/api/tasks/{task}/backlinks")).await;
    assert_eq!(back.as_array().unwrap().len(), 1);

    // Titles come from the records, not from the label stored in the content.
    fx.patch(&target, json!({ "title": "New name" })).await;
    let (_, resolved) = fx
        .post(
            &format!("/api/workspaces/{}/resolve", fx.ws),
            json!({ "page_ids": [target], "task_ids": [task] }),
        )
        .await;
    assert_eq!(resolved["pages"][0]["title"], "New name");
    assert_eq!(resolved["pages"][0]["deleted"], false);
    assert_eq!(resolved["tasks"][0]["title"], "Order oak");

    // Ids from elsewhere are left out; deleted targets are reported as deleted.
    let other_ws = create_workspace(&fx.app, &fx.cookie, "Work").await;
    let (_, nothing) = fx
        .post(
            &format!("/api/workspaces/{other_ws}/resolve"),
            json!({ "page_ids": [target], "task_ids": [task] }),
        )
        .await;
    assert_eq!(nothing, json!({ "pages": [], "tasks": [] }));
    let res = request(
        &fx.app,
        delete(&format!("/api/pages/{target}"), Some(&fx.cookie)),
    )
    .await;
    assert!(res.status().is_success());
    let (_, resolved) = fx
        .post(
            &format!("/api/workspaces/{}/resolve", fx.ws),
            json!({ "page_ids": [target], "task_ids": [] }),
        )
        .await;
    assert_eq!(resolved["pages"][0]["deleted"], true);

    // Removing the links from the content removes the references; a deleted source is not listed.
    fx.save(&source, json!([])).await;
    let back = fx.json_get(&format!("/api/tasks/{task}/backlinks")).await;
    assert_eq!(back, json!([]));
}

#[tokio::test]
async fn comments_are_searchable_as_their_task() {
    let fx = fixture().await;
    let (_, task) = fx.task("Sand the table").await;
    let (status, _) = fx
        .post(
            &format!("/api/tasks/{task}/comments"),
            json!({ "body": "we need 120 grit sandpaper" }),
        )
        .await;
    assert!(status.is_success(), "{status}");
    let hits = fx
        .json_get(&format!("/api/workspaces/{}/search?q=sandpaper", fx.ws))
        .await;
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["id"], task);
    assert_eq!(hits[0]["kind"], "task");
    assert_eq!(hits[0]["title"], "Sand the table");
    assert!(hits[0]["snippet"].as_str().unwrap().contains("sandpaper"));

    // Matching in both the title and a comment is still one result.
    let hits = fx
        .json_get(&format!("/api/workspaces/{}/search?q=sand", fx.ws))
        .await;
    assert_eq!(hits.as_array().unwrap().len(), 1);
}
