use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::{error::ApiError, models::Todo, state::AppState};

#[derive(Deserialize, ToSchema)]
pub struct NewTodo {
    pub title: String,
}

/// List all todos.
#[utoipa::path(
    get,
    path = "/todos",
    tag = "todos",
    responses((status = 200, description = "All todos", body = [Todo]))
)]
pub async fn list_todos(State(state): State<AppState>) -> Result<Json<Vec<Todo>>, ApiError> {
    let db = &mut *state.db.lock().await;
    let todos = Todo::all().exec(db).await?;
    Ok(Json(todos))
}

/// Create a todo.
#[utoipa::path(
    post,
    path = "/todos",
    tag = "todos",
    request_body = NewTodo,
    responses((status = 201, description = "Todo created", body = Todo))
)]
pub async fn create_todo(
    State(state): State<AppState>,
    Json(input): Json<NewTodo>,
) -> Result<(StatusCode, Json<Todo>), ApiError> {
    let db = &mut *state.db.lock().await;
    let todo = toasty::create!(Todo {
        title: input.title,
        done: false,
    })
    .exec(db)
    .await?;
    Ok((StatusCode::CREATED, Json(todo)))
}

/// Fetch a todo by id.
#[utoipa::path(
    get,
    path = "/todos/{id}",
    tag = "todos",
    params(("id" = uuid::Uuid, Path, description = "Todo id")),
    responses(
        (status = 200, description = "The todo", body = Todo),
        (status = 404, description = "No todo with that id")
    )
)]
pub async fn get_todo(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Todo>, ApiError> {
    let db = &mut *state.db.lock().await;
    let todo = Todo::get_by_id(db, &id).await?;
    Ok(Json(todo))
}
