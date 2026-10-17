//! Appearance: workspace theme (admins), personal choice per workspace, and light/dark mode.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::*;

async fn json_of(res: axum::http::Response<axum::body::Body>) -> Value {
    let text = body_string(res.into_body()).await;
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

async fn send(
    app: &axum::Router,
    req: axum::http::Request<axum::body::Body>,
) -> (StatusCode, Value) {
    let res = request(app, req).await;
    (res.status(), json_of(res).await)
}

#[tokio::test]
async fn workspace_themes_personal_choices_and_mode() {
    let (app, db) = app_with_db().await;
    let admin = bootstrap_admin(&app).await;
    let ws = create_workspace(&app, &admin, "Home").await;

    // A second user who is a plain member.
    let db = &mut db.clone();
    let user = toasty::create!(woodmouse::models::User {
        email: "m@home.io".to_string(),
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
        workspace_id: ws,
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
    let member = format!("woodmouse_session={token}");

    let theme_uri = format!("/api/workspaces/{ws}/theme");
    let my_uri = format!("/api/workspaces/{ws}/my-theme");

    // Nothing is set at first.
    let (status, view) = send(&app, get(&theme_uri, Some(&member))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        view,
        json!({ "workspace": { "palette": null, "accent": null }, "mine": { "palette": null, "accent": null } })
    );

    // Only admins set the workspace theme; the accent is stored in lowercase.
    let body = json!({ "palette": "slate", "accent": "#2F5D8A" }).to_string();
    assert_eq!(
        send(&app, put_json(&theme_uri, &body, Some(&member)))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, set) = send(&app, put_json(&theme_uri, &body, Some(&admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(set["accent"], "#2f5d8a");

    // Bad values are refused.
    for bad in [
        json!({ "palette": "neon" }),
        json!({ "accent": "blue" }),
        json!({ "accent": "#12345" }),
    ] {
        let (status, _) = send(&app, put_json(&theme_uri, &bad.to_string(), Some(&admin))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // A member's own choice is separate from the workspace's, and clearing it goes back to the workspace's.
    let mine = json!({ "palette": "mono" }).to_string();
    assert_eq!(
        send(&app, put_json(&my_uri, &mine, Some(&member))).await.0,
        StatusCode::OK
    );
    let (_, view) = send(&app, get(&theme_uri, Some(&member))).await;
    assert_eq!(view["workspace"]["palette"], "slate");
    assert_eq!(view["mine"], json!({ "palette": "mono", "accent": null }));
    let (_, admin_view) = send(&app, get(&theme_uri, Some(&admin))).await;
    assert_eq!(admin_view["mine"]["palette"], Value::Null);
    assert_eq!(
        send(&app, put_json(&my_uri, "{}", Some(&member))).await.0,
        StatusCode::OK
    );
    let (_, view) = send(&app, get(&theme_uri, Some(&member))).await;
    assert_eq!(view["mine"], json!({ "palette": null, "accent": null }));

    // Light or dark is an account setting.
    let (_, me) = send(&app, get("/api/auth/me", Some(&member))).await;
    assert_eq!(me["theme_mode"], "system");
    let (status, me) = send(
        &app,
        patch_json("/api/auth/me", r#"{"theme_mode":"dark"}"#, Some(&member)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["theme_mode"], "dark");
    let (status, _) = send(
        &app,
        patch_json("/api/auth/me", r#"{"theme_mode":"sepia"}"#, Some(&member)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
