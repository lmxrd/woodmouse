//! The file library: upload, duplicate detection, attachments, safe serving, and admin cleanup.

mod common;

use axum::http::StatusCode;
use serde_json::Value;

use common::*;

async fn json(res: axum::http::Response<axum::body::Body>) -> Value {
    serde_json::from_str(&body_string(res.into_body()).await).expect("valid json")
}

struct Fx {
    app: axum::Router,
    db: toasty::Db,
    admin: String,
    ws: uuid::Uuid,
    task: String,
}

async fn fixture_with(app: axum::Router, db: toasty::Db) -> Fx {
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;
    let project = json(
        request(
            &app,
            post_json(
                &format!("/api/workspaces/{ws}/projects"),
                r#"{"name":"P"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = json(
        request(
            &app,
            post_json(
                &format!("/api/projects/{project}/lists"),
                r#"{"name":"L"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = json(
        request(
            &app,
            post_json(
                &format!("/api/lists/{list}/tasks"),
                r#"{"title":"T"}"#,
                Some(&admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    Fx {
        app,
        db,
        admin,
        ws,
        task,
    }
}

async fn fixture() -> Fx {
    let (app, db) = app_with_db().await;
    fixture_with(app, db).await
}

impl Fx {
    async fn upload(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        task: Option<&str>,
    ) -> (StatusCode, Value) {
        let res = request(
            &self.app,
            upload(
                &format!("/api/workspaces/{}/files", self.ws),
                name,
                mime,
                bytes,
                task,
                Some(&self.admin),
            ),
        )
        .await;
        let status = res.status();
        (status, json(res).await)
    }
    async fn files(&self, query: &str) -> Vec<Value> {
        let res = request(
            &self.app,
            get(
                &format!("/api/workspaces/{}/files{query}", self.ws),
                Some(&self.admin),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        json(res).await.as_array().unwrap().clone()
    }
}

#[tokio::test]
async fn upload_download_and_safe_headers() {
    let fx = fixture().await;
    let png = b"\x89PNG\r\n\x1a\nfake image bytes";
    let (status, file) = fx
        .upload("../../etc/cellar door.png", "image/png", png, None)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(file["name"], "cellar door.png", "path parts are stripped");
    assert_eq!(file["previewable"], true);
    assert_eq!(file["size"], png.len());
    assert_eq!(file["used_in"], 0);
    let id = file["id"].as_str().unwrap();

    // Images display inline; the bytes round-trip.
    let res = request(
        &fx.app,
        get(&format!("/api/files/{id}/content"), Some(&fx.admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/png");
    assert!(
        res.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("inline")
    );
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert_eq!(res.into_body().collect_bytes().await, png.to_vec());

    // SVG and HTML are never inline and never served with their own type.
    for (name, mime) in [("x.svg", "image/svg+xml"), ("x.html", "text/html")] {
        let (_, f) = fx
            .upload(
                name,
                mime,
                format!("<script>{name}</script>").as_bytes(),
                None,
            )
            .await;
        let res = request(
            &fx.app,
            get(
                &format!("/api/files/{}/content", f["id"].as_str().unwrap()),
                Some(&fx.admin),
            ),
        )
        .await;
        assert_eq!(f["previewable"], false);
        assert_eq!(res.headers()["content-type"], "application/octet-stream");
        assert!(
            res.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .starts_with("attachment"),
            "{name}"
        );
    }

    // Names are encoded safely in the header.
    let (_, f) = fx
        .upload(
            "quote final \u{00e6}.pdf",
            "application/pdf",
            b"%PDF-1",
            None,
        )
        .await;
    let res = request(
        &fx.app,
        get(
            &format!("/api/files/{}/content", f["id"].as_str().unwrap()),
            Some(&fx.admin),
        ),
    )
    .await;
    let disposition = res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.contains("filename*=UTF-8''quote%20final%20%C3%A6.pdf"),
        "{disposition}"
    );

    // Anonymous access is refused.
    let res = request(&fx.app, get(&format!("/api/files/{id}/content"), None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Search narrows the list.
    assert_eq!(fx.files("?q=CELLAR").await.len(), 1);
    assert_eq!(fx.files("").await.len(), 4);
}

trait CollectBytes {
    async fn collect_bytes(self) -> Vec<u8>;
}
impl CollectBytes for axum::body::Body {
    async fn collect_bytes(self) -> Vec<u8> {
        use http_body_util::BodyExt;
        self.collect().await.unwrap().to_bytes().to_vec()
    }
}

#[tokio::test]
async fn duplicates_are_detected_by_content() {
    let fx = fixture().await;
    let (status, original) = fx
        .upload("quote.pdf", "application/pdf", b"same bytes", None)
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Same content under another name: refused, pointing at the existing file.
    let (status, body) = fx
        .upload(
            "quote-final.pdf",
            "application/pdf",
            b"same bytes",
            Some(&fx.task),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["existing_file"]["id"], original["id"]);
    assert_eq!(body["existing_file"]["name"], "quote.pdf");
    assert_eq!(fx.files("").await.len(), 1, "nothing was stored twice");
    assert_eq!(
        fx.files("").await[0]["used_in"],
        0,
        "and nothing was attached either"
    );

    // Same name, different content: fine.
    let (status, _) = fx
        .upload("quote.pdf", "application/pdf", b"other bytes", None)
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // The same content in another workspace is not a duplicate.
    let other_ws = create_workspace(&fx.app, &fx.admin, "Work").await;
    let res = request(
        &fx.app,
        upload(
            &format!("/api/workspaces/{other_ws}/files"),
            "quote.pdf",
            "application/pdf",
            b"same bytes",
            None,
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn attachments_and_library_survive_task_deletion() {
    let fx = fixture().await;
    let (_, file) = fx
        .upload("worktop.pdf", "application/pdf", b"pdf", Some(&fx.task))
        .await;
    let id = file["id"].as_str().unwrap().to_string();
    assert_eq!(file["used_in"], 1);
    let t = json(
        request(
            &fx.app,
            get(&format!("/api/tasks/{}", fx.task), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(t["attachment_count"], 1);

    let attachments = json(
        request(
            &fx.app,
            get(
                &format!("/api/tasks/{}/attachments", fx.task),
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(attachments[0]["id"], id);
    let usage = json(
        request(
            &fx.app,
            get(&format!("/api/files/{id}/usage"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(usage[0]["kind"], "task");
    assert_eq!(usage[0]["title"], "T");
    assert_eq!(usage[0]["in_trash"], false);

    // Deleting the task keeps the file in the library, still counted as used (it can come back).
    request(
        &fx.app,
        delete(&format!("/api/tasks/{}", fx.task), Some(&fx.admin)),
    )
    .await;
    let files = fx.files("").await;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["used_in"], 1);
    let usage = json(
        request(
            &fx.app,
            get(&format!("/api/files/{id}/usage"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(usage[0]["in_trash"], true);
    request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/trash/task/{}/restore", fx.ws, fx.task),
            "{}",
            Some(&fx.admin),
        ),
    )
    .await;

    // Detach, re-attach (idempotent), detach again.
    let res = request(
        &fx.app,
        delete(
            &format!("/api/tasks/{}/attachments/{id}", fx.task),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(fx.files("?unused=true").await.len(), 1);
    for _ in 0..2 {
        let res = request(
            &fx.app,
            post_json(
                &format!("/api/tasks/{}/attachments", fx.task),
                &format!(r#"{{"file_id":"{id}"}}"#),
                Some(&fx.admin),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    assert_eq!(fx.files("").await[0]["used_in"], 1);
    let res = request(
        &fx.app,
        delete(
            &format!("/api/tasks/{}/attachments/{id}", fx.task),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(
        &fx.app,
        delete(
            &format!("/api/tasks/{}/attachments/{id}", fx.task),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_cleanup_removes_only_unreferenced_files() {
    let fx = fixture().await;
    let (_, used) = fx
        .upload("used.pdf", "application/pdf", b"used", Some(&fx.task))
        .await;
    let (_, lonely) = fx
        .upload("lonely.pdf", "application/pdf", b"lonely!", None)
        .await;
    let lonely_id = lonely["id"].as_str().unwrap();

    let preview = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/files/unused", fx.ws),
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(preview["count"], 1);
    assert_eq!(preview["total_bytes"], 7);

    // Writers cannot run it.
    let writer = {
        let user = toasty::create!(woodmouse::models::User {
            email: "w@home.io".to_string(),
            name: "".to_string(),
            password_hash: "x".to_string(),
            super_admin: false,
            active: true,
        })
        .exec(&mut fx.db.clone())
        .await
        .unwrap();
        toasty::create!(woodmouse::models::WorkspaceMember {
            user_id: user.id,
            workspace_id: fx.ws,
            role: woodmouse::models::Role::Write
        })
        .exec(&mut fx.db.clone())
        .await
        .unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        toasty::create!(woodmouse::models::Session {
            token: token.clone(),
            user_id: user.id,
            expires_at: common::far_future()
        })
        .exec(&mut fx.db.clone())
        .await
        .unwrap();
        format!("woodmouse_session={token}")
    };
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/files/cleanup", fx.ws),
            "{}",
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &fx.app,
        upload(
            &format!("/api/workspaces/{}/files", fx.ws),
            "w.txt",
            "text/plain",
            b"writer upload",
            None,
            Some(&writer),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED, "writers can upload");

    // A file attached to a trashed task still counts as used.
    request(
        &fx.app,
        delete(&format!("/api/tasks/{}", fx.task), Some(&fx.admin)),
    )
    .await;
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/workspaces/{}/files/cleanup", fx.ws),
            "{}",
            Some(&fx.admin),
        ),
    )
    .await;
    let result = json(res).await;
    assert_eq!(result["deleted"], 2, "lonely.pdf and the writer's file");
    assert_eq!(result["freed_bytes"], 7 + 13);

    assert_eq!(fx.files("").await.len(), 1);
    let res = request(
        &fx.app,
        get(&format!("/api/files/{lonely_id}/content"), Some(&fx.admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = request(
        &fx.app,
        get(
            &format!("/api/files/{}/content", used["id"].as_str().unwrap()),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the referenced file is untouched"
    );
}

#[tokio::test]
async fn upload_limit_and_validation() {
    let (app, db) = app_with_upload_limit(1024).await;
    let fx = fixture_with(app, db).await;
    let config = json(request(&fx.app, get("/api/config", Some(&fx.admin))).await).await;
    assert_eq!(config["max_upload_bytes"], 1024);

    let (status, _) = fx
        .upload("ok.bin", "application/octet-stream", &[7u8; 1024], None)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = fx
        .upload("big.bin", "application/octet-stream", &[8u8; 1025], None)
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = fx.upload("empty.txt", "text/plain", b"", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Attaching to a task that does not exist fails and stores nothing.
    let before = fx.files("").await.len();
    let (status, _) = fx
        .upload(
            "x.txt",
            "text/plain",
            b"x",
            Some(&uuid::Uuid::new_v4().to_string()),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(fx.files("").await.len(), before);
}

#[tokio::test]
async fn files_attach_to_wiki_pages_too() {
    let fx = fixture().await;
    let uri = format!("/api/workspaces/{}/files", fx.ws);
    let res = request(
        &fx.app,
        upload(
            &uri,
            "plan.pdf",
            "application/pdf",
            b"%PDF plan",
            None,
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let file = json(res).await["id"].as_str().unwrap().to_string();
    let page = json(
        request(
            &fx.app,
            post_json(
                &format!("/api/workspaces/{}/pages", fx.ws),
                r#"{"title":"Plans"}"#,
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let attach = || {
        request(
            &fx.app,
            post_json(
                &format!("/api/pages/{page}/attachments"),
                &format!(r#"{{"file_id":"{file}"}}"#),
                Some(&fx.admin),
            ),
        )
    };
    assert_eq!(attach().await.status(), StatusCode::OK);
    assert_eq!(attach().await.status(), StatusCode::OK, "twice is harmless");
    let list = json(
        request(
            &fx.app,
            get(&format!("/api/pages/{page}/attachments"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["used_in"], 1);

    let usage = json(
        request(
            &fx.app,
            get(&format!("/api/files/{file}/usage"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(usage[0]["kind"], "page");
    assert_eq!(usage[0]["title"], "Plans");

    let res = request(
        &fx.app,
        delete(
            &format!("/api/pages/{page}/attachments/{file}"),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let list = json(
        request(
            &fx.app,
            get(&format!("/api/pages/{page}/attachments"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn inline_files_attach_to_the_page_and_are_audited() {
    let fx = fixture().await;
    let res = request(
        &fx.app,
        upload(
            &format!("/api/workspaces/{}/files", fx.ws),
            "oak.png",
            "image/png",
            b"\x89PNG oak",
            None,
            Some(&fx.admin),
        ),
    )
    .await;
    let file = json(res).await["id"].as_str().unwrap().to_string();
    let page = json(
        request(
            &fx.app,
            post_json(
                &format!("/api/workspaces/{}/pages", fx.ws),
                r#"{"title":"Kitchen"}"#,
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let save = |body: String| {
        request(
            &fx.app,
            put_json(&format!("/api/pages/{page}/blocks"), &body, Some(&fx.admin)),
        )
    };

    // A file block without a file is refused.
    let res = save(r#"{"blocks":[{"type":"file"}]}"#.to_string()).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    // Only file blocks take a file.
    let res = save(format!(
        r#"{{"blocks":[{{"type":"paragraph","attrs":{{"file_id":"{file}"}}}}]}}"#
    ))
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = save(format!(
        r#"{{"blocks":[{{"type":"file","attrs":{{"file_id":"{file}","label":"oak.png"}}}}]}}"#
    ))
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    save(format!(
        r#"{{"blocks":[{{"type":"file","attrs":{{"file_id":"{file}","label":"oak.png"}}}},{{"type":"paragraph"}}]}}"#
    ))
    .await;

    // Showing it in the page attached it (once), so it counts as used.
    let list = json(
        request(
            &fx.app,
            get(&format!("/api/pages/{page}/attachments"), Some(&fx.admin)),
        )
        .await,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "oak.png");

    // The feed recorded it once, and detaching is recorded too.
    request(
        &fx.app,
        delete(
            &format!("/api/pages/{page}/attachments/{file}"),
            Some(&fx.admin),
        ),
    )
    .await;
    let feed = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/activity", fx.ws),
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await;
    let kinds: Vec<&str> = feed
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "page_file_attached").count(),
        1
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "page_file_detached").count(),
        1
    );
    let attached = feed
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "page_file_attached")
        .unwrap();
    assert_eq!(attached["data"]["file"], "oak.png");
    assert_eq!(attached["data"]["title"], "Kitchen");
}

#[tokio::test]
async fn removing_a_file_block_detaches_it_but_hand_attachments_stay() {
    let fx = fixture().await;
    let uri = format!("/api/workspaces/{}/files", fx.ws);
    let res = request(
        &fx.app,
        upload(
            &uri,
            "oak.png",
            "image/png",
            b"\x89PNG oak",
            None,
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let inline = json(res).await["id"].as_str().unwrap().to_string();
    let res = request(
        &fx.app,
        upload(
            &uri,
            "map.svg",
            "image/svg+xml",
            b"<svg>map</svg>",
            None,
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let manual = json(res).await["id"].as_str().unwrap().to_string();
    let page = json(
        request(
            &fx.app,
            post_json(
                &format!("/api/workspaces/{}/pages", fx.ws),
                r#"{"title":"Trails"}"#,
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Attached by hand: it stays until it is detached.
    let res = request(
        &fx.app,
        post_json(
            &format!("/api/pages/{page}/attachments"),
            &format!(r#"{{"file_id":"{manual}"}}"#),
            Some(&fx.admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let save = |body: String| {
        request(
            &fx.app,
            put_json(&format!("/api/pages/{page}/blocks"), &body, Some(&fx.admin)),
        )
    };
    let attachments = || async {
        json(
            request(
                &fx.app,
                get(&format!("/api/pages/{page}/attachments"), Some(&fx.admin)),
            )
            .await,
        )
        .await
    };

    // Showing it in the page attached it once.
    save(format!(
        r#"{{"blocks":[{{"type":"file","attrs":{{"file_id":"{inline}","label":"oak.png"}}}}]}}"#
    ))
    .await;
    let list = attachments().await;
    assert_eq!(list.as_array().unwrap().len(), 2);

    // Removing the block detaches it; the hand-attached file stays.
    save(r#"{"blocks":[{"type":"paragraph"}]}"#.to_string()).await;
    let list = attachments().await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "map.svg");

    let feed = json(
        request(
            &fx.app,
            get(
                &format!("/api/workspaces/{}/activity", fx.ws),
                Some(&fx.admin),
            ),
        )
        .await,
    )
    .await;
    let entries = feed.as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e["kind"] == "page_file_attached")
            .count(),
        2
    );
    let detached: Vec<&Value> = entries
        .iter()
        .filter(|e| e["kind"] == "page_file_detached")
        .collect();
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0]["data"]["file"], "oak.png");
}
