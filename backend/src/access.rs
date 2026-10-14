//! The single workspace access check. Every handler that touches a
//! workspace-scoped resource must go through [`require_access`] (or
//! [`role_in`] when a plain lookup is needed) — workspace membership is the
//! only v1 access mechanism.

use crate::error::ApiError;
use crate::models::{Role, User, WorkspaceMember};

/// The user's role in the workspace: `None` when not a member. Super admins
/// act as admin everywhere.
pub async fn role_in(
    db: &mut toasty::Db,
    user: &User,
    workspace_id: uuid::Uuid,
) -> Result<Option<Role>, ApiError> {
    if user.super_admin {
        return Ok(Some(Role::Admin));
    }
    // The composite primary key guarantees at most one membership row.
    let membership = WorkspaceMember::filter_by_user_id_and_workspace_id(user.id, workspace_id)
        .first()
        .exec(db)
        .await?;
    Ok(membership.map(|m| m.role))
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
    min_role: Role,
) -> Result<Role, ApiError> {
    match role_in(db, user, workspace_id).await? {
        Some(role) if role >= min_role => Ok(role),
        _ => Err(ApiError::not_found()),
    }
}
