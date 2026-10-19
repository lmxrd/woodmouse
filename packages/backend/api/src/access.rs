//! The single workspace access check. Every handler that touches a
//! workspace-scoped resource must go through [`require_access`] (or one of
//! the `require_*_access` resolvers below).
//!
//! A person's role in a workspace is the highest of: admin if they are the super admin, their
//! `global_role` (access to every workspace given by the super admin), and their membership. Deleted
//! workspaces are invisible to everyone (the super admin restores them from the site trash).

use crate::db::Db;
use crate::db::prelude::*;
use crate::error::ApiError;
use entity::{
    List, Project, Role, Task, User, WikiPage, lists, projects, tasks, users, wiki_pages,
    workspace_members, workspaces,
};

/// The user's role in the workspace: `None` when they have no access (or it is deleted).
pub async fn role_in(
    db: &impl Db,
    user: &User,
    workspace_id: uuid::Uuid,
) -> Result<Option<Role>, ApiError> {
    let Ok(workspace) = workspaces::Entity::find_by_id(workspace_id)
        .one_or_not_found(db)
        .await
    else {
        return Ok(None);
    };
    if workspace.deleted_at.is_some() {
        return Ok(None);
    }
    let membership = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user.id))
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .one(db)
        .await?
        .map(|m| m.role);
    let super_admin = user.super_admin.then_some(Role::Admin);
    Ok([super_admin, user.global_role, membership]
        .into_iter()
        .flatten()
        .max())
}

/// Whether the user can see the workspace (any role). Used to validate task assignees.
pub async fn can_see_workspace(
    db: &impl Db,
    user_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
) -> Result<bool, ApiError> {
    let Ok(user) = users::Entity::find_by_id(user_id)
        .one_or_not_found(db)
        .await
    else {
        return Ok(false);
    };
    Ok(user.active && role_in(db, &user, workspace_id).await?.is_some())
}

/// Require at least `min_role` in the workspace.
///
/// Returns the actual role on success. Both "workspace does not exist" and
/// "no access" map to 404 so callers cannot probe for workspace ids they
/// cannot see.
pub async fn require_access(
    db: &impl Db,
    user: &User,
    workspace_id: uuid::Uuid,
    min_role: Role,
) -> Result<Role, ApiError> {
    match role_in(db, user, workspace_id).await? {
        Some(role) if role >= min_role => Ok(role),
        _ => Err(ApiError::not_found()),
    }
}

/// A live project the caller may access at `min_role`.
pub async fn require_project(
    db: &impl Db,
    user: &User,
    project_id: uuid::Uuid,
    min_role: Role,
) -> Result<Project, ApiError> {
    let project = projects::Entity::find_by_id(project_id)
        .one_or_not_found(db)
        .await?;
    if project.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_access(db, user, project.workspace_id, min_role).await?;
    Ok(project)
}

/// Resolve a project to its workspace and require access. Deleted projects look nonexistent.
/// Returns the workspace id.
pub async fn require_project_access(
    db: &impl Db,
    user: &User,
    project_id: uuid::Uuid,
    min_role: Role,
) -> Result<uuid::Uuid, ApiError> {
    Ok(require_project(db, user, project_id, min_role)
        .await?
        .workspace_id)
}

/// A live list (of a live project) the caller may access, with its project's workspace id.
pub async fn require_list(
    db: &impl Db,
    user: &User,
    list_id: uuid::Uuid,
    min_role: Role,
) -> Result<(List, uuid::Uuid), ApiError> {
    let list = lists::Entity::find_by_id(list_id)
        .one_or_not_found(db)
        .await?;
    if list.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    let workspace = require_project_access(db, user, list.project_id, min_role).await?;
    Ok((list, workspace))
}

/// A live task (in a live list and project) the caller may access, with its workspace id.
pub async fn require_task(
    db: &impl Db,
    user: &User,
    task_id: uuid::Uuid,
    min_role: Role,
) -> Result<(Task, uuid::Uuid), ApiError> {
    let task = tasks::Entity::find_by_id(task_id)
        .one_or_not_found(db)
        .await?;
    if task.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    if let Some(list_id) = task.list_id {
        let list = lists::Entity::find_by_id(list_id)
            .one_or_not_found(db)
            .await?;
        if list.deleted_at.is_some() {
            return Err(ApiError::not_found());
        }
    }
    let workspace = require_project_access(db, user, task.project_id, min_role).await?;
    Ok((task, workspace))
}

/// Resolve a task to its workspace and require access. Returns the workspace id.
pub async fn require_task_access(
    db: &impl Db,
    user: &User,
    task_id: uuid::Uuid,
    min_role: Role,
) -> Result<uuid::Uuid, ApiError> {
    Ok(require_task(db, user, task_id, min_role).await?.1)
}

/// The workspace a project belongs to, deleted or not (for Trash, which must see deleted items).
pub async fn project_workspace(
    db: &impl Db,
    project_id: uuid::Uuid,
) -> Result<uuid::Uuid, ApiError> {
    Ok(projects::Entity::find_by_id(project_id)
        .one_or_not_found(db)
        .await?
        .workspace_id)
}

/// The workspace a list belongs to, deleted or not.
pub async fn list_workspace(db: &impl Db, list_id: uuid::Uuid) -> Result<uuid::Uuid, ApiError> {
    let list = lists::Entity::find_by_id(list_id)
        .one_or_not_found(db)
        .await?;
    project_workspace(db, list.project_id).await
}

/// The workspace a task belongs to, deleted or not.
pub async fn task_workspace(db: &impl Db, task_id: uuid::Uuid) -> Result<uuid::Uuid, ApiError> {
    let task = tasks::Entity::find_by_id(task_id)
        .one_or_not_found(db)
        .await?;
    project_workspace(db, task.project_id).await
}

/// A live page the caller may access. Deleted pages look nonexistent.
pub async fn require_page_access(
    db: &impl Db,
    user: &User,
    page_id: uuid::Uuid,
    min_role: Role,
) -> Result<WikiPage, ApiError> {
    let page = wiki_pages::Entity::find_by_id(page_id)
        .one_or_not_found(db)
        .await?;
    if page.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_access(db, user, page.workspace_id, min_role).await?;
    Ok(page)
}

/// Only the super admin.
pub fn require_super_admin(user: &User) -> Result<(), ApiError> {
    if user.super_admin {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}
