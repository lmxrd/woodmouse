//! The database models. Conventions (see TODO.md and AGENTS.md):
//!
//! - Every date-time is a `jiff::civil::DateTime` meaning UTC (see `crate::time`); task dates are a day at
//!   midnight UTC.
//! - Users' things are soft deleted (`deleted_at`); who deleted them is in the `AuditLog`.
//! - Ordering uses integer positions spaced 20 apart (see `crate::ordering`).
//! - Relations (`#[belongs_to]`) are for typed navigation; Toasty enforces them in its query engine, not as
//!   database constraints. Nothing that has children is ever hard deleted, so its cascades do not fire.

use jiff::civil::DateTime;
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

    /// Light, dark or follow the device: `light`, `dark` or `system`; `None` means `system`.
    pub theme_mode: Option<String>,

    /// IANA timezone (`Europe/Oslo`), filled in from the browser the first time; `None` until then.
    pub timezone: Option<String>,

    /// Argon2id hash (PHC string format).
    pub password_hash: String,

    /// The one site-wide super admin: the first registered user. Above every workspace role, gates the
    /// site admin page, never delegated.
    pub super_admin: bool,

    /// Access to every workspace at this role, given by the super admin; `None` means memberships only.
    pub global_role: Option<Role>,

    /// Disabled accounts cannot sign in; everything they made stays.
    pub active: bool,
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

    #[belongs_to(key = user_id, references = id)]
    pub user: toasty::Deferred<User>,

    pub expires_at: DateTime,
}

/// A one-time invite link for a user who has not set a password yet. The
/// token is shown to the inviting admin in the UI; accepting it sets the
/// password and deletes the invite.
#[derive(Debug, toasty::Model)]
pub struct UserInvite {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[unique]
    pub token: String,

    #[index]
    pub user_id: uuid::Uuid,

    #[belongs_to(key = user_id, references = id)]
    pub user: toasty::Deferred<User>,

    pub expires_at: DateTime,
}

/// A workspace: the top-level container for projects, tasks, and wiki pages.
#[derive(Debug, Clone, Serialize, ToSchema, toasty::Model)]
pub struct Workspace {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    pub name: String,

    pub description: String,

    /// The workspace's colour palette (one of `themes::PALETTES`); `None` means the default.
    pub theme_palette: Option<String>,

    /// The workspace's accent colour as `#rrggbb`; `None` means the palette's own.
    pub theme_accent: Option<String>,

    /// Soft delete: a deleted workspace is hidden from everyone and only the super admin can restore it.
    #[serde(skip)]
    #[schema(ignore)]
    pub deleted_at: Option<DateTime>,
}

/// A user's membership in a workspace. One row per (user, workspace).
#[derive(Debug, toasty::Model)]
#[index(user_id)]
pub struct WorkspaceMember {
    #[key]
    pub user_id: uuid::Uuid,

    #[key]
    pub workspace_id: uuid::Uuid,

    #[belongs_to(key = user_id, references = id)]
    pub user: toasty::Deferred<User>,

    #[belongs_to(key = workspace_id, references = id)]
    pub workspace: toasty::Deferred<Workspace>,

    pub role: Role,

    /// This member's own palette for the workspace; `None` means the workspace's.
    pub theme_palette: Option<String>,

    /// This member's own accent colour for the workspace; `None` means the workspace's.
    pub theme_accent: Option<String>,
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

/// A project inside a workspace. Access inherits from workspace membership.
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct Project {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[belongs_to(key = workspace_id, references = id)]
    #[serde(skip)]
    #[schema(ignore)]
    pub workspace: toasty::Deferred<Workspace>,

    pub name: String,

    pub description: String,

    /// Soft delete: when set the row is hidden everywhere but Trash.
    #[serde(skip)]
    #[schema(ignore)]
    pub deleted_at: Option<DateTime>,
}

/// A list of tasks inside a project. A project's inbox is not a list: it is the tasks without one.
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct List {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub project_id: uuid::Uuid,

    #[belongs_to(key = project_id, references = id)]
    #[serde(skip)]
    #[schema(ignore)]
    pub project: toasty::Deferred<Project>,

    pub name: String,

    /// Ordering among the project's lists.
    pub position: i64,

    /// Soft delete: when set the row is hidden everywhere but Trash.
    #[serde(skip)]
    #[schema(ignore)]
    pub deleted_at: Option<DateTime>,
}

/// A sprint: a dated bucket of a project's tasks (a task is in at most one). It is live until its end,
/// and completed when someone completes it, which moves its unfinished tasks on.
#[derive(Debug, toasty::Model)]
pub struct Sprint {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub project_id: uuid::Uuid,

    #[belongs_to(key = project_id, references = id)]
    pub project: toasty::Deferred<Project>,

    pub name: String,

    /// What the sprint is for; may be empty.
    pub goal: String,

    /// First day (midnight UTC).
    pub start_date: DateTime,

    /// Last day (midnight UTC).
    pub end_date: DateTime,

    /// Set when the sprint was completed.
    pub completed_at: Option<DateTime>,

    /// Soft delete: when set the row is hidden everywhere but Trash.
    pub deleted_at: Option<DateTime>,
}

/// A task. `list_id` is `None` while the task is in the project's inbox; `parent_id` is set for subtasks
/// (any depth), which always share their parent's project and list.
#[derive(Debug, toasty::Model)]
pub struct Task {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub project_id: uuid::Uuid,

    #[belongs_to(key = project_id, references = id)]
    pub project: toasty::Deferred<Project>,

    #[index]
    pub list_id: Option<uuid::Uuid>,

    #[belongs_to(key = list_id, references = id)]
    pub list: toasty::Deferred<Option<List>>,

    pub title: String,

    /// Markdown.
    pub description: String,

    /// When the task was completed; `None` while it is open.
    pub completed_at: Option<DateTime>,

    /// Ordering among the tasks of its list (or inbox).
    pub position: i64,

    pub priority: Priority,

    /// A calendar day (midnight UTC), if set.
    pub start_date: Option<DateTime>,

    /// A calendar day (midnight UTC), if set.
    pub due_date: Option<DateTime>,

    /// The sprint it is planned in, if any.
    #[index]
    pub sprint_id: Option<uuid::Uuid>,

    #[belongs_to(key = sprint_id, references = id)]
    pub sprint: toasty::Deferred<Option<Sprint>>,

    /// How long the task is expected to take, in minutes.
    pub estimate_minutes: Option<i64>,

    /// How long has been spent on it so far, in minutes.
    pub spent_minutes: Option<i64>,

    /// The parent task when this is a subtask.
    #[index]
    pub parent_id: Option<uuid::Uuid>,

    /// Soft delete: when set the row is hidden everywhere but Trash.
    pub deleted_at: Option<DateTime>,
}

/// A workspace-scoped label that tasks can carry.
#[derive(Debug, Clone, Serialize, ToSchema, toasty::Model)]
pub struct Label {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[belongs_to(key = workspace_id, references = id)]
    #[serde(skip)]
    #[schema(ignore)]
    pub workspace: toasty::Deferred<Workspace>,

    pub name: String,

    /// Hex color like `#71717a`.
    pub color: String,

    /// Soft delete: when set the row is hidden everywhere but Trash.
    #[serde(skip)]
    #[schema(ignore)]
    pub deleted_at: Option<DateTime>,
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

    #[belongs_to(key = task_id, references = id)]
    pub task: toasty::Deferred<Task>,

    #[belongs_to(key = user_id, references = id)]
    pub user: toasty::Deferred<User>,
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

    #[belongs_to(key = task_id, references = id)]
    pub task: toasty::Deferred<Task>,

    #[belongs_to(key = label_id, references = id)]
    pub label: toasty::Deferred<Label>,
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

/// A comment on a task. The body is markdown; earlier bodies are `Revision`s.
#[derive(Debug, toasty::Model)]
pub struct Comment {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub task_id: uuid::Uuid,

    #[belongs_to(key = task_id, references = id)]
    pub task: toasty::Deferred<Task>,

    pub author_id: uuid::Uuid,

    pub body: String,

    pub created_at: DateTime,

    pub deleted_at: Option<DateTime>,
}

/// A file in a workspace's library. The bytes live in the storage backend named by
/// `storage_backend` under `storage_key`. Files are never deleted along with whatever references
/// them; only an admin's cleanup of unreferenced files removes them.
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

    pub created_at: DateTime,

    /// Soft delete: when set the file is hidden everywhere but Trash.
    pub deleted_at: Option<DateTime>,
}

/// One version of a file's content. The file row always describes its newest version; older ones are
/// kept (and downloadable) until an admin's cleanup removes an unreferenced file.
#[derive(Debug, toasty::Model)]
pub struct FileVersion {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub file_id: uuid::Uuid,

    #[belongs_to(key = file_id, references = id)]
    pub file: toasty::Deferred<File>,

    /// 1 for the first upload, then counting up.
    pub number: i64,

    /// The name the file had when this version was uploaded.
    pub name: String,

    pub mime: String,

    pub size: i64,

    pub sha256: String,

    pub storage_backend: String,

    pub storage_key: String,

    pub uploaded_by: uuid::Uuid,

    pub created_at: DateTime,
}

/// A file attached to a task or a wiki page. Attached at most once per target.
#[derive(Debug, toasty::Model)]
#[index(file_id)]
#[index(target_id)]
pub struct FileRef {
    #[key]
    pub file_id: uuid::Uuid,

    /// `task` or `page`.
    #[key]
    pub target_type: String,

    #[key]
    pub target_id: uuid::Uuid,

    /// Page attachments only: the page's content shows the file (a file block), so the reference
    /// follows the content and removing the block detaches it. Attached by hand it stays until
    /// it is detached. Always false for tasks.
    pub embedded: bool,
}

/// Who may see an audit entry: the workspace's activity feed (and the audit) or the audit only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, toasty::Embed)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Workspace,
    Admin,
}

/// One entry of the append-only audit log, which is also the source of the workspace activity feed.
/// Never updated or deleted. `data` is a JSON object with what the UI needs to phrase the entry (titles
/// are snapshotted; edits point to their `Revision`), never content itself. No network addresses or
/// browsers are recorded.
#[derive(Debug, toasty::Model)]
#[index(workspace_id, visibility, created_at)]
pub struct AuditLog {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub actor_id: Option<uuid::Uuid>,

    /// snake_case: `login`, `task_created`, `entity_deleted`, ...
    #[index]
    pub action: String,

    /// `user`, `workspace`, `task`, `page`, `comment`, ...
    pub target_type: Option<String>,

    #[index]
    pub target_id: Option<uuid::Uuid>,

    /// Set for workspace events (always when `visibility` is `workspace`).
    pub workspace_id: Option<uuid::Uuid>,

    pub visibility: Visibility,

    /// JSON object text.
    pub data: String,

    pub created_at: DateTime,
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

    #[belongs_to(key = project_id, references = id)]
    pub project: toasty::Deferred<Project>,

    pub owner_id: uuid::Uuid,

    pub name: String,

    /// JSON object text.
    pub params: String,

    pub is_shared: bool,

    pub created_at: DateTime,

    pub deleted_at: Option<DateTime>,
}

/// A wiki page. Pages form a tree per workspace through `parent_id`; siblings are ordered by `position`.
/// `content` is the page's blocks as versioned JSON (see `crate::blocks::Content`).
#[derive(Debug, toasty::Model)]
pub struct WikiPage {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub workspace_id: uuid::Uuid,

    #[belongs_to(key = workspace_id, references = id)]
    pub workspace: toasty::Deferred<Workspace>,

    #[index]
    pub parent_id: Option<uuid::Uuid>,

    pub title: String,

    /// Ordering among siblings.
    pub position: i64,

    /// `{"version": 1, "blocks": [...]}`.
    pub content: String,

    pub created_by: uuid::Uuid,

    pub created_at: DateTime,

    pub updated_by: uuid::Uuid,

    /// The last title or content change.
    pub updated_at: DateTime,

    /// Counts content saves; a save based on an older version is refused as a conflict.
    pub version: i64,

    pub deleted_at: Option<DateTime>,
}

/// A version of a task (title and description), wiki page (title and blocks) or comment (body), with who
/// made it. A revision holds the content *after* a change; a page's newest revision is updated in place
/// while one editing session lasts. Admins can redact a revision's content.
#[derive(Debug, toasty::Model)]
pub struct Revision {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    /// `task`, `page` or `comment`.
    pub target_type: String,

    #[index]
    pub target_id: uuid::Uuid,

    pub actor_id: uuid::Uuid,

    pub created_at: DateTime,

    /// The last change in this revision's editing session.
    pub updated_at: DateTime,

    /// JSON; empty after redaction.
    pub content: String,

    pub redacted_at: Option<DateTime>,
}

/// A reference from a page's content to a page or task (`@task` mention or `[[page]]` link), derived from
/// the blocks each time they are saved. It powers "Referenced by" and never holds anything the blocks
/// do not.
#[derive(Debug, toasty::Model)]
pub struct ContentLink {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub source_page_id: uuid::Uuid,

    #[belongs_to(key = source_page_id, references = id)]
    pub source_page: toasty::Deferred<WikiPage>,

    #[index]
    pub target_id: uuid::Uuid,

    /// `page` or `task`.
    pub target_kind: String,
}

/// The live editing state of a page: its Yjs document, so that a restart does not lose what clients still
/// hold. `epoch` names the document's lineage; it changes whenever the document is rebuilt from the
/// page's content (after a save outside live editing, or a restore), which tells clients to start over
/// instead of merging two unrelated histories.
#[derive(Debug, toasty::Model)]
pub struct PageDoc {
    #[key]
    pub page_id: uuid::Uuid,

    #[belongs_to(key = page_id, references = id)]
    pub page: toasty::Deferred<WikiPage>,

    pub epoch: String,

    /// The document as a Yjs update, base64.
    pub state: String,

    pub updated_at: DateTime,
}

/// A secret link to a read-only calendar feed (iCalendar) of tasks with dates. Calendar apps cannot log
/// in, so the secret in the link is the credential; whoever has it sees those tasks, as long as the
/// person who made it still has access to them. Revoking sets `revoked_at` (links are never deleted).
#[derive(Debug, toasty::Model)]
pub struct CalendarLink {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    /// Whose link it is; the feed shows what this person can see.
    #[index]
    pub user_id: uuid::Uuid,

    #[belongs_to(key = user_id, references = id)]
    pub user: toasty::Deferred<User>,

    /// `user` (tasks assigned to the person, in every workspace), `project` or `workspace`.
    pub scope: String,

    pub workspace_id: Option<uuid::Uuid>,

    pub project_id: Option<uuid::Uuid>,

    /// A label the person gave it, or what it shows.
    pub name: String,

    /// `read` for now; reserved for write access (CalDAV) later.
    pub mode: String,

    #[unique]
    pub secret: String,

    pub created_at: DateTime,

    /// The last time a calendar app fetched it (rounded to a few minutes).
    pub last_used_at: Option<DateTime>,

    pub revoked_at: Option<DateTime>,
}

/// Someone watching a task or a wiki page: they are notified of what happens to it.
#[derive(Debug, toasty::Model)]
#[index(user_id)]
#[index(target_id)]
pub struct Watch {
    #[key]
    pub user_id: uuid::Uuid,

    /// `task` or `page`.
    #[key]
    pub target_type: String,

    #[key]
    pub target_id: uuid::Uuid,

    pub created_at: DateTime,
}

/// Something that happened that a person should hear about. Created together with the change (see
/// `crate::notify`), read in the app and optionally emailed.
#[derive(Debug, toasty::Model)]
#[index(user_id, created_at)]
pub struct Notification {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    pub user_id: uuid::Uuid,

    /// `task_assigned`, `comment_added`, `mentioned`, `task_changed`, `page_changed`, `due_soon`, `overdue`.
    pub kind: String,

    pub workspace_id: uuid::Uuid,

    /// `task` or `page`, with its id: what the notification opens.
    pub target_type: Option<String>,
    pub target_id: Option<uuid::Uuid>,

    /// Who caused it (never the recipient).
    pub actor_id: Option<uuid::Uuid>,

    /// JSON object with what the UI needs to phrase it (titles at the time, project id, ...).
    pub data: String,

    pub created_at: DateTime,

    pub read_at: Option<DateTime>,

    /// When the instant-email sweep dealt with it (sent, or skipped because they do not want it).
    pub emailed_at: Option<DateTime>,
}

/// A person's notification preferences. In-app notifications are always on.
#[derive(Debug, toasty::Model)]
pub struct NotificationSetting {
    #[key]
    pub user_id: uuid::Uuid,

    /// Email each notification as it happens.
    pub email_instant: bool,

    /// Email a daily summary.
    pub digest: bool,

    /// The local hour (0-23, in the person's timezone) the digest is sent.
    pub digest_hour: i64,

    /// When the last digest was sent.
    pub last_digest_at: Option<DateTime>,
}

/// The site's SMTP settings, edited by the super admin. Each field that is set here overrides the
/// matching environment variable (`SMTP_HOST`, ...). One row, id 1.
#[derive(Debug, toasty::Model)]
pub struct EmailSettings {
    #[key]
    pub id: i64,

    pub host: Option<String>,

    pub port: Option<i64>,

    pub username: Option<String>,

    /// Stored as given: whoever can read the database can read it.
    pub password: Option<String>,

    /// The address mail is sent from.
    pub sender: Option<String>,

    /// `starttls`, `tls` or `none`.
    pub tls: Option<String>,
}
