//! Appearance: a workspace's palette and accent colour (set by admins), each member's own choice for the
//! workspace, and the light/dark mode (an account setting, see `UpdateMe`).

use crate::db::prelude::*;
use crate::models::{workspace_members, workspaces};
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::Role;
use crate::state::AppState;

/// The colour palettes the app ships (each has a light and a dark version).
pub const PALETTES: [&str; 4] = ["paper_pine", "slate", "mono", "lavender"];
/// Light, dark, or follow the device.
pub const MODES: [&str; 3] = ["light", "dark", "system"];

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_theme))
        .routes(routes!(set_workspace_theme))
        .routes(routes!(set_my_theme))
}

/// A palette and accent; either can be absent (use the palette's default accent, or the next level up).
#[derive(Serialize, Deserialize, ToSchema, Default, Clone)]
pub struct ThemeChoice {
    /// One of `paper_pine`, `slate`, `mono`, `lavender`.
    pub palette: Option<String>,
    /// `#rrggbb`.
    pub accent: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ThemeView {
    /// What admins set for the workspace.
    pub workspace: ThemeChoice,
    /// What you set for yourself in this workspace. If either field is set it replaces the workspace's
    /// theme as a whole (an absent accent then means the palette's own); both absent means the
    /// workspace's theme applies.
    pub mine: ThemeChoice,
}

fn valid(choice: &ThemeChoice) -> Result<(), ApiError> {
    if let Some(palette) = &choice.palette
        && !PALETTES.contains(&palette.as_str())
    {
        return Err(ApiError::bad_request("unknown palette"));
    }
    if let Some(accent) = &choice.accent {
        let hex = accent.strip_prefix('#').unwrap_or("");
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ApiError::bad_request("the accent must look like #1f4d3a"));
        }
    }
    Ok(())
}

/// Lowercase the accent so the same colour is always stored the same way.
fn normalized(choice: ThemeChoice) -> ThemeChoice {
    ThemeChoice {
        palette: choice.palette,
        accent: choice.accent.map(|a| a.to_lowercase()),
    }
}

/// The workspace's theme and your own choice for it.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/theme",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The theme", body = ThemeView),
        (status = 404, description = "No such workspace, or not a member")
    )
)]
pub async fn get_theme(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<ThemeView>, ApiError> {
    let db = state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    let workspace = workspaces::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await?;
    let mine = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user.id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(db)
        .await?
        .map(|m| ThemeChoice {
            palette: m.theme_palette,
            accent: m.theme_accent,
        })
        .unwrap_or_default();
    Ok(Json(ThemeView {
        workspace: ThemeChoice {
            palette: workspace.theme_palette,
            accent: workspace.theme_accent,
        },
        mine,
    }))
}

/// Set the workspace's palette and accent for everyone who has not chosen their own (admins only).
/// Absent fields are cleared.
#[utoipa::path(
    put,
    path = "/workspaces/{id}/theme",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = ThemeChoice,
    responses(
        (status = 200, description = "The workspace theme", body = ThemeChoice),
        (status = 400, description = "Unknown palette or invalid accent"),
        (status = 404, description = "No such workspace, or not an admin")
    )
)]
pub async fn set_workspace_theme(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<ThemeChoice>,
) -> Result<Json<ThemeChoice>, ApiError> {
    valid(&input)?;
    let input = normalized(input);
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    let workspace = workspaces::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    workspaces::ActiveModel {
        id: Unchanged(workspace.id),
        theme_palette: Set(input.palette.clone()),
        theme_accent: Set(input.accent.clone()),
        ..Default::default()
    }
    .update(&*db)
    .await?;
    Ok(Json(input))
}

/// Set your own palette and accent for this workspace (an absent accent means the palette's own). Sending
/// neither goes back to the workspace's theme.
#[utoipa::path(
    put,
    path = "/workspaces/{id}/my-theme",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = ThemeChoice,
    responses(
        (status = 200, description = "Your choice", body = ThemeChoice),
        (status = 400, description = "Unknown palette, invalid accent, or you are not a member"),
        (status = 404, description = "No such workspace, or no access")
    )
)]
pub async fn set_my_theme(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<ThemeChoice>,
) -> Result<Json<ThemeChoice>, ApiError> {
    valid(&input)?;
    let input = normalized(input);
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Read).await?;
    let Some(member) = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user.id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(&*db)
        .await?
    else {
        return Err(ApiError::bad_request(
            "only members can set their own theme for a workspace",
        ));
    };
    workspace_members::Entity::update_many()
        .set(workspace_members::ActiveModel {
            theme_palette: Set(input.palette.clone()),
            theme_accent: Set(input.accent.clone()),
            ..Default::default()
        })
        .filter(workspace_members::Column::UserId.eq(member.user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(member.workspace_id))
        .exec(&*db)
        .await?;
    Ok(Json(input))
}
