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

    /// Display name; may be empty (the UI then shows the email).
    pub name: String,

    /// Preferred date format; `None` means the default (`YYYY-MM-DD`).
    pub date_format: Option<String>,

    /// Preferred clock, `24h` or `12h`; `None` means the default (`24h`).
    pub time_format: Option<String>,

    /// First day of the week in calendars, `monday` or `sunday`; `None` means the default (`monday`).
    pub week_start: Option<String>,

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

    /// Soft delete: when set (Unix epoch seconds) the row is hidden everywhere but Trash.
    #[serde(skip)]
    pub deleted_at: Option<i64>,

    /// Who deleted it.
    #[serde(skip)]
    pub deleted_by: Option<uuid::Uuid>,
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

    /// The project's inbox: an unorganized dump list every project starts with. New quick-added
    /// tasks land here, and it cannot be deleted.
    pub is_inbox: bool,

    /// Soft delete: when set (Unix epoch seconds) the row is hidden everywhere but Trash.
    #[serde(skip)]
    pub deleted_at: Option<i64>,

    /// Who deleted it.
    #[serde(skip)]
    pub deleted_by: Option<uuid::Uuid>,
}

/// A task. `position` is a float for fractional reordering; `parent_id` is
/// used for subtasks (any depth).
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

    /// The parent task when this is a subtask (any depth). A subtask lives in its parent's list.
    pub parent_id: Option<uuid::Uuid>,

    /// When the title or description last changed (unset for tasks never edited since they were created).
    pub text_edited_at: Option<i64>,

    pub text_edited_by: Option<uuid::Uuid>,

    /// Soft delete: when set (Unix epoch seconds) the row is hidden everywhere but Trash.
    #[serde(skip)]
    pub deleted_at: Option<i64>,

    /// Who deleted it.
    #[serde(skip)]
    pub deleted_by: Option<uuid::Uuid>,
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

    /// Soft delete: when set (Unix epoch seconds) the row is hidden everywhere but Trash.
    #[serde(skip)]
    pub deleted_at: Option<i64>,

    /// Who deleted it.
    #[serde(skip)]
    pub deleted_by: Option<uuid::Uuid>,
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

/// How two tasks relate. `Blocks` and `Duplicates` read from the owning task to the related one
/// ("A blocks B"); `RelatesTo` is symmetric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, toasty::Embed)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Blocks,
    RelatesTo,
    Duplicates,
}

/// A link between two tasks of the same workspace.
#[derive(Debug, toasty::Model)]
pub struct TaskLink {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    /// The task that owns the link ("this task blocks ...").
    #[index]
    pub task_id: uuid::Uuid,

    #[index]
    pub related_task_id: uuid::Uuid,

    pub kind: LinkKind,
}

/// A comment on a task. The body is markdown.
#[derive(Debug, toasty::Model)]
pub struct Comment {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub task_id: uuid::Uuid,

    pub author_id: uuid::Uuid,

    pub body: String,

    /// Unix epoch seconds.
    pub created_at: i64,

    /// Unix epoch seconds of the last edit, if any.
    pub edited_at: Option<i64>,

    pub deleted_at: Option<i64>,

    pub deleted_by: Option<uuid::Uuid>,
}

/// A file in a workspace's library. The bytes live in the storage backend named by
/// `storage_backend` under `storage_key`. Files are never deleted along with whatever references
/// them; only an admin's cleanup of unreferenced files (or deleting the workspace) removes them.
#[derive(Debug, toasty::Model)]
pub struct File {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    pub name: String,

    pub mime: String,

    /// Size in bytes.
    pub size: i64,

    /// Hex SHA-256 of the content; identical content in a workspace is stored once.
    #[index]
    pub sha256: String,

    pub storage_backend: String,

    pub storage_key: String,

    pub uploaded_by: uuid::Uuid,

    /// Unix epoch seconds.
    pub created_at: i64,
}

/// A file attached to a task. Composite key: attached at most once.
#[derive(Debug, toasty::Model)]
#[index(file_id)]
#[index(task_id)]
pub struct FileRef {
    #[key]
    pub file_id: uuid::Uuid,

    #[key]
    pub task_id: uuid::Uuid,
}

/// One entry of the activity feed: who did what, to which task, when. `kind` is a short snake_case
/// name (`task_created`, `comment_added`, ...) and `data` a JSON object with what the UI needs to
/// phrase it (titles are snapshotted, so entries stay readable after renames and deletes).
#[derive(Debug, toasty::Model)]
pub struct Activity {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[index]
    pub project_id: Option<uuid::Uuid>,

    #[index]
    pub task_id: Option<uuid::Uuid>,

    pub actor_id: uuid::Uuid,

    pub kind: String,

    /// JSON text.
    pub data: String,

    /// Unix epoch seconds.
    pub created_at: i64,
}

/// A named set of view settings (view, filters, sort) for a project, private to its owner or shared
/// with the workspace. `params` is a JSON object the frontend interprets (the same keys as the
/// project URL's search params).
#[derive(Debug, toasty::Model)]
pub struct SavedView {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[index]
    pub project_id: uuid::Uuid,

    pub owner_id: uuid::Uuid,

    pub name: String,

    /// JSON object text.
    pub params: String,

    pub is_shared: bool,

    /// Unix epoch seconds.
    pub created_at: i64,

    pub deleted_at: Option<i64>,

    pub deleted_by: Option<uuid::Uuid>,
}

/// A wiki page. Pages form a tree per workspace through `parent_id`; siblings are ordered by `position`.
/// The content lives in [`Block`] rows.
#[derive(Debug, toasty::Model)]
pub struct Page {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[index]
    pub parent_id: Option<uuid::Uuid>,

    pub title: String,

    /// Ordering among siblings (floats, so a move only changes the moved page).
    pub position: f64,

    pub created_by: uuid::Uuid,

    /// Unix epoch seconds.
    pub created_at: i64,

    pub updated_by: uuid::Uuid,

    /// Unix epoch seconds of the last title or content change.
    pub updated_at: i64,

    /// Counts content saves; a save based on an older version is refused as a conflict.
    pub version: i64,

    pub deleted_at: Option<i64>,

    pub deleted_by: Option<uuid::Uuid>,
}

/// One block of a page's content, in `position` order. `attrs` and `spans` are JSON text in the
/// [`crate::blocks`] format. Saving a page replaces its blocks; history is kept as revisions.
#[derive(Debug, toasty::Model)]
pub struct Block {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub page_id: uuid::Uuid,

    /// A [`crate::blocks::BlockKind`] name.
    pub kind: String,

    pub position: f64,

    pub attrs: String,

    pub spans: String,
}

/// An earlier state of a page's content, kept when an editing session by someone else (or after a pause)
/// begins. See `revisions.rs` for when versions are taken.
#[derive(Debug, toasty::Model)]
pub struct PageRevision {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub page_id: uuid::Uuid,

    /// Unix epoch seconds when this content was saved.
    pub saved_at: i64,

    pub saved_by: uuid::Uuid,

    /// The page's title at that time.
    pub title: String,

    /// The blocks, as JSON (`Vec<BlockData>`).
    pub blocks: String,

    /// Unix epoch seconds when this version was recorded.
    pub created_at: i64,
}

/// An earlier title and description of a task.
#[derive(Debug, toasty::Model)]
pub struct TaskRevision {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub task_id: uuid::Uuid,

    /// When this text was saved; unset for the text the task was created with.
    pub saved_at: Option<i64>,

    pub saved_by: Option<uuid::Uuid>,

    pub title: String,

    pub description: String,

    /// Unix epoch seconds when this version was recorded.
    pub created_at: i64,
}
