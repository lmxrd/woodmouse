//! Calendar links: secret URLs to a read-only iCalendar feed of tasks with dates. People manage their own
//! links; calendar apps fetch the feed without logging in, so the secret in the URL is the credential.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::ical::{self, FeedTask};
use crate::models::{
    CalendarLink, List, Priority, Project, Role, Task, TaskAssignee, User, Workspace,
};
use crate::state::AppState;

/// Links are shown to the owner only; the feed itself is the public part.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(
            list_calendar_links,
            create_calendar_link,
            revoke_calendar_links
        ))
        .routes(routes!(revoke_calendar_link))
}

pub fn public_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(feed))
}

/// How much a link shows.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkScope {
    /// Tasks assigned to the person, in every workspace they belong to.
    User,
    /// Every task in one project.
    Project,
    /// Every task in one workspace.
    Workspace,
}

impl LinkScope {
    fn as_str(self) -> &'static str {
        match self {
            LinkScope::User => "user",
            LinkScope::Project => "project",
            LinkScope::Workspace => "workspace",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "user" => LinkScope::User,
            "project" => LinkScope::Project,
            "workspace" => LinkScope::Workspace,
            _ => return None,
        })
    }
}

#[derive(Serialize, ToSchema)]
pub struct CalendarLinkView {
    pub id: uuid::Uuid,
    pub scope: LinkScope,
    pub name: String,
    pub workspace_id: Option<uuid::Uuid>,
    pub workspace_name: Option<String>,
    pub project_id: Option<uuid::Uuid>,
    pub project_name: Option<String>,
    /// `read` (the only mode so far).
    pub mode: String,
    /// The feed's path on this server; put the server's address in front of it.
    pub path: String,
    /// Unix epoch seconds.
    pub created_at: i64,
    /// Unix epoch seconds; absent if no calendar app has fetched it yet.
    pub last_used_at: Option<i64>,
}

fn feed_path(secret: &str) -> String {
    format!("/api/calendar/{secret}.ics")
}

async fn view_of(db: &mut toasty::Db, link: &CalendarLink) -> Result<CalendarLinkView, ApiError> {
    let workspace_name = match link.workspace_id {
        Some(id) => Workspace::get_by_id(db, &id).await.ok().map(|w| w.name),
        None => None,
    };
    let project_name = match link.project_id {
        Some(id) => Project::get_by_id(db, &id).await.ok().map(|p| p.name),
        None => None,
    };
    Ok(CalendarLinkView {
        id: link.id,
        scope: LinkScope::parse(&link.scope).unwrap_or(LinkScope::User),
        name: link.name.clone(),
        workspace_id: link.workspace_id,
        workspace_name,
        project_id: link.project_id,
        project_name,
        mode: link.mode.clone(),
        path: feed_path(&link.secret),
        created_at: link.created_at,
        last_used_at: link.last_used_at,
    })
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LinkFilter {
    /// Only links for this workspace (including its projects).
    pub workspace_id: Option<uuid::Uuid>,
    /// Only links for this project.
    pub project_id: Option<uuid::Uuid>,
}

fn matches(link: &CalendarLink, filter: &LinkFilter) -> bool {
    filter
        .workspace_id
        .is_none_or(|w| link.workspace_id == Some(w))
        && filter.project_id.is_none_or(|p| link.project_id == Some(p))
}

/// Your calendar links, newest first.
#[utoipa::path(
    get,
    path = "/calendar-links",
    tag = "calendar",
    security(("session_cookie" = [])),
    params(LinkFilter),
    responses((status = 200, description = "Your active links", body = Vec<CalendarLinkView>))
)]
pub async fn list_calendar_links(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(filter): Query<LinkFilter>,
) -> Result<Json<Vec<CalendarLinkView>>, ApiError> {
    let db = &mut *state.db.lock().await;
    let mut links: Vec<CalendarLink> = CalendarLink::filter_by_user_id(user.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.revoked_at.is_none() && matches(l, &filter))
        .collect();
    links.sort_by_key(|l| std::cmp::Reverse((l.created_at, l.id)));
    let mut out = vec![];
    for link in &links {
        out.push(view_of(db, link).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateLink {
    pub scope: LinkScope,
    /// Required for `project` and `workspace`.
    pub workspace_id: Option<uuid::Uuid>,
    /// Required for `project`.
    pub project_id: Option<uuid::Uuid>,
    /// A label for the link; defaults to what it shows.
    pub name: Option<String>,
}

const MAX_NAME: usize = 100;

/// Make a calendar link for what you can see.
#[utoipa::path(
    post,
    path = "/calendar-links",
    tag = "calendar",
    security(("session_cookie" = [])),
    request_body = CreateLink,
    responses(
        (status = 201, description = "The new link", body = CalendarLinkView),
        (status = 400, description = "Missing or invalid scope"),
        (status = 404, description = "No such workspace or project, or no access")
    )
)]
pub async fn create_calendar_link(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<CreateLink>,
) -> Result<(StatusCode, Json<CalendarLinkView>), ApiError> {
    let db = &mut *state.db.lock().await;
    let (workspace_id, project_id, default_name) = match input.scope {
        LinkScope::User => (None, None, "My tasks".to_string()),
        LinkScope::Workspace => {
            let id = input
                .workspace_id
                .ok_or_else(|| ApiError::bad_request("a workspace link needs workspace_id"))?;
            access::require_access(db, &user, id, Role::Read).await?;
            let name = Workspace::get_by_id(db, &id).await?.name;
            (Some(id), None, name)
        }
        LinkScope::Project => {
            let id = input
                .project_id
                .ok_or_else(|| ApiError::bad_request("a project link needs project_id"))?;
            let workspace = access::require_project_access(db, &user, id, Role::Read).await?;
            let name = Project::get_by_id(db, &id).await?.name;
            (Some(workspace), Some(id), name)
        }
    };
    let name = input.name.as_deref().map(str::trim).unwrap_or("");
    if name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!(
            "name can be at most {MAX_NAME} characters"
        )));
    }
    let link = toasty::create!(CalendarLink {
        user_id: user.id,
        scope: input.scope.as_str().to_string(),
        workspace_id,
        project_id,
        name: if name.is_empty() {
            default_name
        } else {
            name.to_string()
        },
        mode: "read".to_string(),
        // 256 random bits: the secret is the whole credential.
        secret: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        created_at: auth::now_secs(),
    })
    .exec(db)
    .await?;
    Ok((StatusCode::CREATED, Json(view_of(db, &link).await?)))
}

/// Revoke one of your links. The URL stops working at once.
#[utoipa::path(
    delete,
    path = "/calendar-links/{id}",
    tag = "calendar",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Link id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 404, description = "No such link")
    )
)]
pub async fn revoke_calendar_link(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    let mut link = CalendarLink::get_by_id(db, &id).await?;
    if link.user_id != user.id || link.revoked_at.is_some() {
        return Err(ApiError::not_found());
    }
    link.update()
        .revoked_at(Some(auth::now_secs()))
        .exec(db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Revoke all of your links, or only those for a workspace or project.
#[utoipa::path(
    delete,
    path = "/calendar-links",
    tag = "calendar",
    security(("session_cookie" = [])),
    params(LinkFilter),
    responses((status = 204, description = "Revoked"))
)]
pub async fn revoke_calendar_links(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(filter): Query<LinkFilter>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    let now = auth::now_secs();
    for mut link in CalendarLink::filter_by_user_id(user.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.revoked_at.is_none() && matches(l, &filter))
    {
        link.update().revoked_at(Some(now)).exec(db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A task a feed shows, and where it lives.
struct Source {
    task: Task,
    project_id: uuid::Uuid,
    project_name: String,
    workspace_id: uuid::Uuid,
}

/// The tasks a link shows, with their projects.
async fn tasks_for(
    db: &mut toasty::Db,
    link: &CalendarLink,
    owner: &User,
) -> Result<Vec<Source>, ApiError> {
    let mut projects: Vec<Project> = match LinkScope::parse(&link.scope) {
        Some(LinkScope::Project) => match link.project_id {
            Some(id) => vec![Project::get_by_id(db, &id).await?],
            None => vec![],
        },
        Some(LinkScope::Workspace) => match link.workspace_id {
            Some(id) => Project::filter_by_workspace_id(id).exec(db).await?,
            None => vec![],
        },
        _ => vec![],
    };
    let mut tasks: Vec<Task> = vec![];
    if LinkScope::parse(&link.scope) == Some(LinkScope::User) {
        let ids: Vec<uuid::Uuid> = TaskAssignee::filter_by_user_id(owner.id)
            .exec(db)
            .await?
            .into_iter()
            .map(|a| a.task_id)
            .collect();
        if !ids.is_empty() {
            tasks = Task::all()
                .filter(in_list(Task::fields().id(), ids))
                .filter(Task::fields().deleted_at().is_none())
                .exec(db)
                .await?;
        }
    } else {
        projects.retain(|p| p.deleted_at.is_none());
        let project_ids: Vec<uuid::Uuid> = projects.iter().map(|p| p.id).collect();
        if project_ids.is_empty() {
            return Ok(vec![]);
        }
        let lists = List::all()
            .filter(in_list(List::fields().project_id(), project_ids))
            .exec(db)
            .await?;
        let list_ids: Vec<uuid::Uuid> = lists
            .into_iter()
            .filter(|l| l.deleted_at.is_none())
            .map(|l| l.id)
            .collect();
        if !list_ids.is_empty() {
            tasks = Task::all()
                .filter(in_list(Task::fields().list_id(), list_ids))
                .filter(Task::fields().deleted_at().is_none())
                .exec(db)
                .await?;
        }
    }

    // Resolve each task's list and project once; keep only what the owner can still see.
    let mut lists: HashMap<uuid::Uuid, List> = HashMap::new();
    let mut known: HashMap<uuid::Uuid, Project> = projects.into_iter().map(|p| (p.id, p)).collect();
    let mut allowed: HashMap<uuid::Uuid, bool> = HashMap::new();
    let mut out = vec![];
    for task in tasks {
        if task.start_date.is_none() && task.due_date.is_none() {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = lists.entry(task.list_id) {
            slot.insert(List::get_by_id(db, &task.list_id).await?);
        }
        let list = &lists[&task.list_id];
        if list.deleted_at.is_some() {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = known.entry(list.project_id) {
            slot.insert(Project::get_by_id(db, &list.project_id).await?);
        }
        let project = &known[&list.project_id];
        if project.deleted_at.is_some() {
            continue;
        }
        let workspace = project.workspace_id;
        let can_see = match allowed.get(&workspace) {
            Some(ok) => *ok,
            None => {
                let ok = access::role_in(db, owner, workspace).await?.is_some();
                allowed.insert(workspace, ok);
                ok
            }
        };
        if can_see {
            out.push(Source {
                project_id: project.id,
                project_name: project.name.clone(),
                workspace_id: workspace,
                task,
            });
        }
    }
    Ok(out)
}

/// Where the app is reached from, for the links inside events.
fn public_base(headers: &HeaderMap) -> String {
    if let Ok(url) = std::env::var("PUBLIC_URL") {
        return url.trim_end_matches('/').to_string();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    format!("{scheme}://{host}")
}

/// A feed is fetched every so often; remember that it was, but not on every request.
const USED_RESOLUTION_SECS: i64 = 300;

/// The calendar feed behind a link (iCalendar). No login: the secret in the path is the credential.
#[utoipa::path(
    get,
    path = "/calendar/{token}",
    tag = "calendar",
    params(("token" = String, Path, description = "The link's secret with a `.ics` ending")),
    responses(
        (status = 200, description = "An iCalendar file", body = String, content_type = "text/calendar"),
        (status = 404, description = "Unknown or revoked link, or its owner can no longer see it")
    )
)]
pub async fn feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let secret = token.strip_suffix(".ics").unwrap_or(&token);
    let base = public_base(&headers);
    let db = &mut *state.db.lock().await;
    let Ok(mut link) = CalendarLink::get_by_secret(db, secret).await else {
        return Err(ApiError::not_found());
    };
    if link.revoked_at.is_some() {
        return Err(ApiError::not_found());
    }
    let owner = User::get_by_id(db, &link.user_id).await?;
    // The owner must still be able to see what the link shows.
    let visible = match (LinkScope::parse(&link.scope), link.workspace_id) {
        (Some(LinkScope::User), _) => true,
        (_, Some(workspace)) => access::role_in(db, &owner, workspace).await?.is_some(),
        _ => false,
    };
    if !visible {
        return Err(ApiError::not_found());
    }
    let tasks = tasks_for(db, &link, &owner).await?;
    let now = auth::now_secs();
    if link
        .last_used_at
        .is_none_or(|t| now - t >= USED_RESOLUTION_SECS)
    {
        link.update().last_used_at(Some(now)).exec(db).await?;
    }

    let feed: Vec<FeedTask> = tasks
        .into_iter()
        .map(
            |Source {
                 task,
                 project_id,
                 project_name,
                 workspace_id,
             }| FeedTask {
                url: format!("{base}/w/{workspace_id}/p/{project_id}/t/{}", task.id),
                priority: match task.priority {
                    Priority::Low => "low",
                    Priority::Medium => "medium",
                    Priority::High => "high",
                    Priority::Urgent => "urgent",
                },
                id: task.id,
                title: task.title,
                description: task.description,
                project: project_name,
                start: task.start_date,
                due: task.due_date,
                completed: task.is_completed,
            },
        )
        .collect();
    let body = ical::calendar(&link.name, &feed, now);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
            (header::CACHE_CONTROL, "private, max-age=300"),
        ],
        body,
    )
        .into_response())
}
