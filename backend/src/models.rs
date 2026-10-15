use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Workspace role. Variant order defines the privilege order: read < write <
/// admin. The toasty `Embed` derive stores the snake_case variant label
/// (`read`, `write`, `admin`) — same string the API uses.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
    toasty::Embed,
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

/// A one-time invite link for a user who has not set a password yet. The
/// token is shown to the inviting admin in the UI; accepting it sets the
/// password and deletes the invite.
#[derive(Debug, toasty::Model)]
pub struct Invite {
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

/// Task priority. Variant order defines the urgency order.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
    toasty::Embed,
)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

/// A project inside a workspace. Access inherits from workspace membership —
/// there are no project members in v1.
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct Project {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    pub name: String,
}

/// A list of tasks inside a project.
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct List {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub project_id: uuid::Uuid,

    pub name: String,
}

/// A task. `position` is a float for fractional reordering; `parent_id` is
/// reserved for Phase 2 subtasks (nullable now so no migration is needed).
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct Task {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub list_id: uuid::Uuid,

    pub title: String,

    /// Plain text for now; rich text once the wiki editor (Model A blocks)
    /// lands in Phase 3.
    pub description: String,

    pub is_completed: bool,

    /// When the task was completed (Unix epoch seconds); set and cleared together with
    /// `is_completed`, so a Done view can show the most recent first.
    pub completed_at: Option<i64>,

    /// Ordering within the list; new tasks append (max + 1).
    pub position: f64,

    pub priority: Priority,

    /// Start date as Unix epoch seconds, if set (for timeline views).
    pub start_date: Option<i64>,

    /// Due date as Unix epoch seconds, if set.
    pub due_date: Option<i64>,

    /// Reserved for Phase 2 subtasks.
    pub parent_id: Option<uuid::Uuid>,
}

/// A workspace-scoped label that tasks can carry.
#[derive(Debug, Clone, Serialize, ToSchema, toasty::Model)]
pub struct Label {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    pub name: String,

    /// Hex color like `#71717a`.
    pub color: String,
}

/// Task assignee. Composite key: a user is assigned to a task at most once.
#[derive(Debug, toasty::Model)]
#[index(task_id)]
#[index(user_id)]
pub struct TaskAssignee {
    #[key]
    pub task_id: uuid::Uuid,

    #[key]
    pub user_id: uuid::Uuid,
}

/// Task/label association. Composite key: a label is attached at most once.
#[derive(Debug, toasty::Model)]
#[index(task_id)]
#[index(label_id)]
pub struct TaskLabel {
    #[key]
    pub task_id: uuid::Uuid,

    #[key]
    pub label_id: uuid::Uuid,
}
