use serde::Serialize;
use utoipa::ToSchema;

/// Workspace role. Variant order defines the privilege order: read < write <
/// admin. The toasty `Embed` derive stores the snake_case variant label
/// (`read`, `write`, `admin`) — same string the API uses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema, toasty::Embed,
)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Read,
    Write,
    Admin,
}

/// A user. Never serialized directly (it carries the password hash); API
/// responses go through the `PublicUser` DTO.
#[derive(Debug, toasty::Model)]
pub struct User {
    /// Primary key; auto-filled on insert with a time-ordered UUID v7.
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[unique]
    pub email: String,

    pub name: String,

    /// Argon2id hash (PHC string format).
    pub password_hash: String,

    /// System-level super admin: the first registered user, sees everything.
    pub super_admin: bool,
}

/// A login session. The opaque token travels in an httpOnly cookie.
#[derive(Debug, toasty::Model)]
pub struct Session {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[unique]
    pub token: String,

    #[index]
    pub user_id: uuid::Uuid,

    /// Expiry as Unix epoch seconds.
    pub expires_at: i64,
}

/// A workspace: the top-level container for projects, tasks, and wiki pages.
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct Workspace {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    pub name: String,
}

/// A user's membership in a workspace. In v1 this is the only access
/// mechanism — project-level membership is deferred.
///
/// One row per (user, workspace): the composite primary key makes duplicate
/// memberships impossible at the schema level. `ProjectMember`, if it ever
/// arrives, copies this shape.
#[derive(Debug, toasty::Model)]
#[index(user_id)]
pub struct WorkspaceMember {
    #[key]
    pub user_id: uuid::Uuid,

    #[key]
    pub workspace_id: uuid::Uuid,

    pub role: Role,
}
