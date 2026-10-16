//! The single workspace access check. Every handler that touches a
//! workspace-scoped resource must go through [`require_access`] (or one of
//! the `require_*_access` resolvers below) — workspace membership is the
//! only v1 access mechanism.

use crate::error::ApiError;
use crate::models::{List, Page, Project, Task, User, WorkspaceMember};

/// The user's role in the workspace: `None` when not a member. Super admins
/// act as admin everywhere.
pub async fn role_in(
    db: &mut toasty::Db,
    user: &User,
    workspace_id: uuid::Uuid,
) -> Result<Option<crate::models::Role>, ApiError> {
    if user.super_admin {
        return Ok(Some(crate::models::Role::Admin));
    }
    // The composite primary key guarantees at most one membership row.
    let membership = WorkspaceMember::filter_by_user_id_and_workspace_id(user.id, workspace_id)
        .first()
        .exec(db)
        .await?;
    Ok(membership.map(|m| m.role))
}

/// Whether the user is a workspace member with any role. Used to validate
/// task assignees, who must be able to see the workspace.
pub async fn is_workspace_member(
    db: &mut toasty::Db,
    user_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
) -> Result<bool, ApiError> {
    Ok(
        WorkspaceMember::filter_by_user_id_and_workspace_id(user_id, workspace_id)
            .first()
            .exec(db)
            .await?
            .is_some(),
    )
}

/// Require at least `min_role` in the workspace.
///
/// Returns the actual role on success. Both "workspace does not exist" and
/// "no access" map to 404 so callers cannot probe for workspace ids they
/// are not a member of.
pub async fn require_access(
    db: &mut toasty::Db,
    user: &User,
    workspace_id: uuid::Uuid,
    min_role: crate::models::Role,
) -> Result<crate::models::Role, ApiError> {
    match role_in(db, user, workspace_id).await? {
        Some(role) if role >= min_role => Ok(role),
        _ => Err(ApiError::not_found()),
    }
}

/// Resolve a project to its workspace and require access. Deleted projects look nonexistent.
/// Returns the workspace id.
pub async fn require_project_access(
    db: &mut toasty::Db,
    user: &User,
    project_id: uuid::Uuid,
    min_role: crate::models::Role,
) -> Result<uuid::Uuid, ApiError> {
    let project = Project::get_by_id(db, &project_id).await?;
    if project.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_access(db, user, project.workspace_id, min_role).await?;
    Ok(project.workspace_id)
}

/// Resolve a list to its workspace (via the project) and require access. Deleted lists, and lists
/// of deleted projects, look nonexistent.
pub async fn require_list_access(
    db: &mut toasty::Db,
    user: &User,
    list_id: uuid::Uuid,
    min_role: crate::models::Role,
) -> Result<uuid::Uuid, ApiError> {
    let list = List::get_by_id(db, &list_id).await?;
    if list.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_project_access(db, user, list.project_id, min_role).await
}

/// Resolve a task to its workspace (via list and project) and require access. Deleted tasks look
/// nonexistent.
pub async fn require_task_access(
    db: &mut toasty::Db,
    user: &User,
    task_id: uuid::Uuid,
    min_role: crate::models::Role,
) -> Result<uuid::Uuid, ApiError> {
    let task = Task::get_by_id(db, &task_id).await?;
    if task.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_list_access(db, user, task.list_id, min_role).await
}

/// The workspace a project belongs to, deleted or not (for Trash, which must see deleted items).
pub async fn project_workspace(
    db: &mut toasty::Db,
    project_id: uuid::Uuid,
) -> Result<uuid::Uuid, ApiError> {
    Ok(Project::get_by_id(db, &project_id).await?.workspace_id)
}

/// The workspace a list belongs to, deleted or not.
pub async fn list_workspace(
    db: &mut toasty::Db,
    list_id: uuid::Uuid,
) -> Result<uuid::Uuid, ApiError> {
    let list = List::get_by_id(db, &list_id).await?;
    project_workspace(db, list.project_id).await
}

/// The workspace a task belongs to, deleted or not.
pub async fn task_workspace(
    db: &mut toasty::Db,
    task_id: uuid::Uuid,
) -> Result<uuid::Uuid, ApiError> {
    let task = Task::get_by_id(db, &task_id).await?;
    list_workspace(db, task.list_id).await
}

/// A live page the caller may access. Deleted pages look nonexistent.
pub async fn require_page_access(
    db: &mut toasty::Db,
    user: &User,
    page_id: uuid::Uuid,
    min_role: crate::models::Role,
) -> Result<Page, ApiError> {
    let page = Page::get_by_id(db, &page_id).await?;
    if page.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    require_access(db, user, page.workspace_id, min_role).await?;
    Ok(page)
}
