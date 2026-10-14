//! Integration tests: auth flow against the real router and an in-memory
//! SQLite database.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use woodmouse::{routes, state::AppState};

async fn app() -> axum::Router {
    app_with_db().await.0
}

async fn app_with_db() -> (axum::Router, toasty::Db) {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    db.push_schema().await.expect("create schema");

    // The clone shares the pool, so it sees the same in-memory database the
    // router uses — the tests can seed users and sessions directly.
    let state = AppState::new(db.clone());
    let router = routes::api(state.clone())
        .with_state(AppState::new(db.clone()))
        .split_for_parts()
        .0;
    (router, db)
}

async fn body_string(body: Body) -> String {
    let bytes = body.collect().await.expect("read body").to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

fn cookie_of(res: &axum::http::Response<Body>) -> String {
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .expect("session cookie in response")
        .to_str()
        .expect("ascii cookie header");
    set_cookie
        .split(';')
        .next()
        .expect("cookie value")
        .to_string()
}

async fn request(app: &axum::Router, req: Request<Body>) -> axum::http::Response<Body> {
    app.clone().oneshot(req).await.expect("request succeeds")
}

fn post_json(uri: &str, json: &str, cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    builder.body(Body::from(json.to_string())).unwrap()
}

fn get(uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    builder.body(Body::empty()).unwrap()
}

const BOOTSTRAP: &str = r#"{"email":"admin@home.io","name":"Admin","password":"correct horse"}"#;

#[tokio::test]
async fn health_probes() {
    let app = app().await;
    let res = request(&app, get("/api/health", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("\"status\":\"ok\""), "body: {body}");
    assert!(body.contains("\"db\":\"ok\""), "body: {body}");
}

#[tokio::test]
async fn bootstrap_login_logout_flow() {
    let app = app().await;

    // Before any user exists: bootstrap required.
    let res = request(&app, get("/api/auth/status", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("\"bootstrap_required\":true"), "body: {body}");

    // Bootstrap creates the super admin and logs in.
    let res = request(&app, post_json("/api/auth/bootstrap", BOOTSTRAP, None)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let cookie = cookie_of(&res);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("admin@home.io"), "body: {body}");
    assert!(body.contains("\"super_admin\":true"), "body: {body}");

    // Bootstrap is closed once a user exists.
    let res = request(
        &app,
        post_json(
            "/api/auth/bootstrap",
            r#"{"email":"x@y.io","name":"X","password":"longenough"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let res = request(&app, get("/api/auth/status", None)).await;
    let body = body_string(res.into_body()).await;
    assert!(
        body.contains("\"bootstrap_required\":false"),
        "body: {body}"
    );

    // The session cookie works.
    let res = request(&app, get("/api/auth/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("admin@home.io"), "body: {body}");

    // Without a cookie: 401.
    let res = request(&app, get("/api/auth/me", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Wrong password: 401, same generic error as unknown email.
    let res = request(
        &app,
        post_json(
            "/api/auth/login",
            r#"{"email":"admin@home.io","password":"wrong pass"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = request(
        &app,
        post_json(
            "/api/auth/login",
            r#"{"email":"nobody@home.io","password":"wrong pass"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Correct credentials: a fresh session.
    let res = request(
        &app,
        post_json(
            "/api/auth/login",
            r#"{"email":"admin@home.io","password":"correct horse"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie2 = cookie_of(&res);

    // Logout invalidates only the logged-out session.
    let res = request(&app, post_json("/api/auth/logout", "", Some(&cookie2))).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = request(&app, get("/api/auth/me", Some(&cookie2))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = request(&app, get("/api/auth/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn bootstrap_validates_input() {
    let app = app().await;

    let res = request(
        &app,
        post_json(
            "/api/auth/bootstrap",
            r#"{"email":"nope","name":"A","password":"longenough"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = request(
        &app,
        post_json(
            "/api/auth/bootstrap",
            r#"{"email":"a@b.io","name":"A","password":"short"}"#,
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspaces_access_is_enforced() {
    let (app, db) = app_with_db().await;

    // First user: bootstrap super admin.
    let res = request(&app, post_json("/api/auth/bootstrap", BOOTSTRAP, None)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let admin_cookie = cookie_of(&res);

    // Unauthenticated access is rejected.
    let res = request(&app, get("/api/workspaces", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // A garbage session cookie is rejected by the auth layer, not just a
    // missing one — enforcement is fail-closed for every protected route.
    let res = request(
        &app,
        get(
            "/api/workspaces",
            Some("woodmouse_session=definitely-not-a-real-token"),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = request(
        &app,
        get(
            "/api/auth/me",
            Some("woodmouse_session=definitely-not-a-real-token"),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // The admin creates a workspace.
    let res = request(
        &app,
        post_json("/api/workspaces", r#"{"name":"Home"}"#, Some(&admin_cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_string(res.into_body()).await;
    let workspace_id: uuid::Uuid = serde_json::from_slice(
        &serde_json::from_str::<serde_json::Value>(&body)
            .expect("valid json")
            .get("id")
            .expect("id in response")
            .to_string()
            .into_bytes(),
    )
    .expect("uuid id");
    assert!(body.contains("Home"), "body: {body}");

    // The admin sees it, both in the list and by id.
    let res = request(&app, get("/api/workspaces", Some(&admin_cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(body.contains("Home"), "body: {body}");

    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}"),
            Some(&admin_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // A second, non-member user seeded directly in the database.
    let other = toasty::create!(woodmouse::models::User {
        email: "other@home.io".to_string(),
        name: "Other".to_string(),
        password_hash: "not-a-login-hash".to_string(),
        super_admin: false,
    })
    .exec(&mut db.clone())
    .await
    .expect("seed second user");
    let other_token = uuid::Uuid::new_v4().to_string();
    toasty::create!(woodmouse::models::Session {
        token: other_token.clone(),
        user_id: other.id,
        expires_at: 4_102_444_800, // 2100-01-01
    })
    .exec(&mut db.clone())
    .await
    .expect("seed second session");
    let other_cookie = format!("woodmouse_session={other_token}");

    // Membership filtering: the list shows no workspaces.
    let res = request(&app, get("/api/workspaces", Some(&other_cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert_eq!(body, "[]", "no workspaces visible: {body}");

    // Direct access to the workspace: 404 (same as nonexistent — no leak).
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}"),
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // A nonexistent workspace gives the same status.
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{}", uuid::Uuid::new_v4()),
            Some(&admin_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // The second user can create their own workspace and sees only that.
    let res = request(
        &app,
        post_json(
            "/api/workspaces",
            r#"{"name":"Other place"}"#,
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = request(&app, get("/api/workspaces", Some(&other_cookie))).await;
    let body = body_string(res.into_body()).await;
    assert!(body.contains("Other place"), "body: {body}");
    assert!(!body.contains("Home"), "body: {body}");

    // Empty name is rejected.
    let res = request(
        &app,
        post_json("/api/workspaces", r#"{"name":"  "}"#, Some(&admin_cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // The composite primary key rejects a second membership row for the
    // same (user, workspace) pair — role lookups stay deterministic.
    let dup = toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: other.id,
        workspace_id: workspace_id,
        role: woodmouse::models::Role::Read,
    })
    .exec(&mut db.clone())
    .await;
    let dup2 = toasty::create!(woodmouse::models::WorkspaceMember {
        user_id: other.id,
        workspace_id: workspace_id,
        role: woodmouse::models::Role::Admin,
    })
    .exec(&mut db.clone())
    .await;
    assert!(dup.is_ok(), "first membership insert should succeed");
    assert!(dup2.is_err(), "duplicate membership must be rejected");
    // And granting the second user read access now opens the workspace.
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace_id}"),
            Some(&other_cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}
