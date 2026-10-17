//! Sprints: dated buckets of a project's tasks. A task is in at most one sprint; completing a sprint
//! moves its unfinished tasks to the backlog or another open sprint.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::{limited, required_max};
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Project, Role, Sprint, Task};
use crate::routes::tasks::validate_sprint;
use crate::state::AppState;
use crate::{time, trash};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_sprints, create_sprint))
        .routes(routes!(update_sprint, delete_sprint))
        .routes(routes!(complete_sprint))
}

const MAX_NAME: usize = 100;
const MAX_GOAL: usize = 1000;

/// A sprint with a summary of its tasks.
#[derive(Serialize, ToSchema)]
pub struct SprintView {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub name: String,
    pub goal: String,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub start_date: DateTime,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub end_date: DateTime,
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub completed_at: Option<DateTime>,
    /// Tasks planned in it (top-level and subtasks).
    pub task_count: usize,
    /// How many of them are done.
    pub done_count: usize,
    /// The sum of their estimates, in minutes.
    pub estimate_minutes: i64,
}

fn view(sprint: &Sprint, tasks: &[Task]) -> SprintView {
    let own: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.sprint_id == Some(sprint.id) && t.deleted_at.is_none())
        .collect();
    SprintView {
        id: sprint.id,
        project_id: sprint.project_id,
        name: sprint.name.clone(),
        goal: sprint.goal.clone(),
        start_date: sprint.start_date,
        end_date: sprint.end_date,
        completed_at: sprint.completed_at,
        task_count: own.len(),
        done_count: own.iter().filter(|t| t.completed_at.is_some()).count(),
        estimate_minutes: own.iter().filter_map(|t| t.estimate_minutes).sum(),
    }
}

fn check_dates(start: DateTime, end: DateTime) -> Result<(), ApiError> {
    if end < start {
        return Err(ApiError::bad_request(
            "a sprint cannot end before it starts",
        ));
    }
    Ok(())
}

/// A live sprint the caller may access at `min_role`, with its project.
async fn load(
    db: &mut dyn toasty::Executor,
    user: &crate::models::User,
    id: uuid::Uuid,
    min_role: Role,
) -> Result<(Sprint, Project), ApiError> {
    let sprint = Sprint::get_by_id(db, &id).await?;
    if sprint.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    let project = access::require_project(db, user, sprint.project_id, min_role).await?;
    Ok((sprint, project))
}

/// The project's sprints, by start date (completed ones included).
#[utoipa::path(
    get,
    path = "/projects/{project_id}/sprints",
    tag = "sprints",
    security(("session_cookie" = [])),
    params(("project_id" = uuid::Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "The project's sprints", body = [SprintView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn list_sprints(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<SprintView>>, ApiError> {
    let db = &mut state.db();
    access::require_project(db, &user, project_id, Role::Read).await?;
    let tasks = Task::filter_by_project_id(project_id).exec(db).await?;
    let mut sprints: Vec<Sprint> = Sprint::filter_by_project_id(project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|s| s.deleted_at.is_none())
        .collect();
    sprints.sort_by_key(|s| (s.start_date, s.id));
    Ok(Json(sprints.iter().map(|s| view(s, &tasks)).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateSprint {
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub start_date: DateTime,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub end_date: DateTime,
}

/// Create a sprint in a project.
#[utoipa::path(
    post,
    path = "/projects/{project_id}/sprints",
    tag = "sprints",
    security(("session_cookie" = [])),
    params(("project_id" = uuid::Uuid, Path, description = "Project id")),
    request_body = CreateSprint,
    responses(
        (status = 201, description = "Sprint created", body = SprintView),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn create_sprint(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<uuid::Uuid>,
    Json(input): Json<CreateSprint>,
) -> Result<(StatusCode, Json<SprintView>), ApiError> {
    let name = required_max(&input.name, "name", MAX_NAME)?;
    let goal = limited(&input.goal, "goal", MAX_GOAL)?;
    let (start, end) = (time::to_day(input.start_date), time::to_day(input.end_date));
    check_dates(start, end)?;

    let mut db = state.writer().await;
    let project = access::require_project(&mut *db, &user, project_id, Role::Write).await?;
    let mut tx = db.transaction().await?;
    let sprint = toasty::create!(Sprint {
        project_id,
        name: name.clone(),
        goal,
        start_date: start,
        end_date: end,
    })
    .exec(&mut tx)
    .await?;
    audit::record(
        &mut tx,
        Entry::feed(project.workspace_id, user.id, "sprint_created")
            .target("sprint", sprint.id)
            .data(serde_json::json!({ "project_id": project_id, "name": name })),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view(&sprint, &[]))))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateSprint {
    pub name: Option<String>,
    pub goal: Option<String>,
    #[serde(default, with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub start_date: Option<DateTime>,
    #[serde(default, with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub end_date: Option<DateTime>,
}

/// Rename a sprint, change its goal or its dates.
#[utoipa::path(
    patch,
    path = "/sprints/{id}",
    tag = "sprints",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Sprint id")),
    request_body = UpdateSprint,
    responses(
        (status = 200, description = "Sprint updated", body = SprintView),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No sprint with that id, or no access")
    )
)]
pub async fn update_sprint(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateSprint>,
) -> Result<Json<SprintView>, ApiError> {
    let mut db = state.writer().await;
    let (mut sprint, _) = load(&mut *db, &user, id, Role::Write).await?;
    let name = input
        .name
        .as_deref()
        .map(|n| required_max(n, "name", MAX_NAME))
        .transpose()?;
    let goal = input
        .goal
        .as_deref()
        .map(|g| limited(g, "goal", MAX_GOAL))
        .transpose()?;
    let start = input
        .start_date
        .map(time::to_day)
        .unwrap_or(sprint.start_date);
    let end = input.end_date.map(time::to_day).unwrap_or(sprint.end_date);
    check_dates(start, end)?;

    if name.is_some() || goal.is_some() || input.start_date.is_some() || input.end_date.is_some() {
        let mut update = sprint.update().start_date(start).end_date(end);
        if let Some(name) = name {
            update = update.name(name);
        }
        if let Some(goal) = goal {
            update = update.goal(goal);
        }
        update.exec(&mut *db).await?;
    }
    let tasks = Task::filter_by_project_id(sprint.project_id)
        .exec(&mut *db)
        .await?;
    Ok(Json(view(&sprint, &tasks)))
}

/// Delete a sprint (it moves to the trash). Its tasks go back to the backlog.
#[utoipa::path(
    delete,
    path = "/sprints/{id}",
    tag = "sprints",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Sprint id")),
    responses(
        (status = 204, description = "Sprint deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No sprint with that id, or no access")
    )
)]
pub async fn delete_sprint(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut db = state.writer().await;
    let (sprint, project) = load(&mut *db, &user, id, Role::Write).await?;
    let mut tx = db.transaction().await?;
    trash::delete_sprint(&mut tx, &sprint, time::now()).await?;
    audit::deleted(
        &mut tx,
        user.id,
        project.workspace_id,
        "sprint",
        id,
        &sprint.name,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct CompleteSprint {
    /// Where unfinished tasks go: another open sprint of the project; absent or `null` is the backlog.
    pub move_to: Option<uuid::Uuid>,
}

/// Complete a sprint. Finished tasks stay in it as its record; unfinished ones move to the backlog or to
/// another open sprint.
#[utoipa::path(
    post,
    path = "/sprints/{id}/complete",
    tag = "sprints",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Sprint id")),
    request_body = CompleteSprint,
    responses(
        (status = 200, description = "Sprint completed", body = SprintView),
        (status = 400, description = "The target sprint is not one of the project's"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No sprint with that id, or no access"),
        (status = 409, description = "Already completed, or the target is completed")
    )
)]
pub async fn complete_sprint(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CompleteSprint>,
) -> Result<Json<SprintView>, ApiError> {
    let mut db = state.writer().await;
    let (mut sprint, project) = load(&mut *db, &user, id, Role::Write).await?;
    if sprint.completed_at.is_some() {
        return Err(ApiError::conflict("this sprint is already completed"));
    }
    if let Some(target) = input.move_to {
        if target == id {
            return Err(ApiError::bad_request("choose another sprint"));
        }
        validate_sprint(&mut *db, sprint.project_id, target).await?;
    }
    let mut tx = db.transaction().await?;
    let tasks = Task::filter_by_project_id(sprint.project_id)
        .exec(&mut tx)
        .await?;
    let unfinished: Vec<uuid::Uuid> = tasks
        .iter()
        .filter(|t| t.sprint_id == Some(id) && t.deleted_at.is_none() && t.completed_at.is_none())
        .map(|t| t.id)
        .collect();
    for task_id in &unfinished {
        Task::filter_by_id(*task_id)
            .update()
            .sprint_id(input.move_to)
            .exec(&mut tx)
            .await?;
    }
    sprint
        .update()
        .completed_at(Some(time::now()))
        .exec(&mut tx)
        .await?;
    audit::record(
        &mut tx,
        Entry::feed(project.workspace_id, user.id, "sprint_completed")
            .target("sprint", id)
            .data(serde_json::json!({
                "project_id": project.id,
                "name": sprint.name,
                "moved": unfinished.len(),
            })),
    )
    .await?;
    tx.commit().await?;
    let tasks = Task::filter_by_project_id(sprint.project_id)
        .exec(&mut *db)
        .await?;
    Ok(Json(view(&sprint, &tasks)))
}
