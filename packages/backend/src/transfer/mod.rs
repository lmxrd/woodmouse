//! Moving data in and out: a JSON document of chosen parts of a workspace, and a CSV of tasks that holds
//! everything needed to set up projects and tasks. Both formats are Woodmouse's own; both are imported
//! through one plan (`Export`), which can be previewed (`dry_run`) before anything is written. An import is
//! atomic: everything is written in one transaction, or nothing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::blocks::BlockData;
use crate::models::Role;
use crate::time;

pub const VERSION: u32 = 1;

/// A workspace (or the chosen parts of one) as a document.
#[derive(Serialize, Deserialize, ToSchema, Default)]
pub struct Export {
    pub version: u32,
    /// When it was exported (RFC 3339).
    #[serde(default)]
    pub exported_at: String,
    pub workspace: Option<ExportWorkspace>,
    pub labels: Option<Vec<ExportLabel>>,
    /// For reference: importing never creates people from this list (people named by tasks and comments are
    /// mapped when importing).
    pub members: Option<Vec<ExportMember>>,
    pub projects: Option<Vec<ExportProject>>,
    pub wiki: Option<Vec<ExportPage>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportWorkspace {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportLabel {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportMember {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub role: Role,
}

#[derive(Serialize, Deserialize, ToSchema, Default)]
pub struct ExportProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// List names, in order.
    #[serde(default)]
    pub lists: Vec<String>,
    #[serde(default)]
    pub sprints: Vec<ExportSprint>,
    #[serde(default)]
    pub tasks: Vec<ExportTask>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone)]
pub struct ExportSprint {
    pub name: String,
    #[serde(default)]
    pub goal: String,
    /// `YYYY-MM-DD` or RFC 3339.
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Default)]
pub struct ExportTask {
    /// A number unique within the project, for `parent` to refer to. Parents come before their subtasks.
    pub id: u32,
    pub parent: Option<u32>,
    /// The list name; absent is the project's inbox.
    pub list: Option<String>,
    pub sprint: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// `low`, `medium`, `high` or `urgent`.
    pub priority: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub estimate_minutes: Option<i64>,
    pub spent_minutes: Option<i64>,
    /// When it was completed (RFC 3339), or `true`; absent means open.
    pub completed: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Emails of members.
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub comments: Vec<ExportComment>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportComment {
    /// The author's email.
    pub author: String,
    pub body: String,
    pub created_at: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportPage {
    pub id: u32,
    pub parent: Option<u32>,
    pub title: String,
    #[serde(default)]
    pub blocks: Vec<BlockData>,
}

/// A person in the file who is not a member of the workspace.
#[derive(Serialize, ToSchema, Debug, Clone)]
pub struct UnmappedUser {
    pub email: String,
    /// How many assignments and comments name them.
    pub references: usize,
}

/// An account the import created, with its one-time invite token.
#[derive(Serialize, ToSchema, Debug)]
pub struct CreatedInvite {
    pub email: String,
    pub invite_token: String,
}

/// What to do about one person in the file who is not a member of the workspace.
#[derive(Deserialize, ToSchema, Debug, Clone)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UserChoice {
    /// Use a member of this workspace instead (their tasks and comments become theirs).
    Map { user_id: uuid::Uuid },
    /// Create the account (or, if it exists, add it to this workspace) with this role. Needs a workspace
    /// admin; an account created this way gets an invite link.
    Create { role: Role },
    /// Leave their tasks unassigned and make you the author of their comments.
    Skip,
}

/// How an import runs.
pub struct ImportOptions {
    /// Only report what would happen; write nothing.
    pub dry_run: bool,
    /// The choice for each unmapped person, by lowercase email.
    pub user_map: HashMap<String, UserChoice>,
    /// Whether the importer may create accounts and memberships (a workspace admin).
    pub can_create_users: bool,
}

/// What an import did (or, for a preview, would do).
#[derive(Serialize, ToSchema, Default, Debug)]
pub struct ImportReport {
    pub dry_run: bool,
    pub projects_created: usize,
    pub lists_created: usize,
    pub sprints_created: usize,
    pub labels_created: usize,
    pub tasks_created: usize,
    pub comments_created: usize,
    pub pages_created: usize,
    /// Things in the file that cannot be imported as they are (unreadable rows, orphaned subtasks, bad
    /// dates, ...). While there are any, nothing is imported.
    pub problems: Vec<String>,
    /// People the file names (assignees, comment authors) who are not members of this workspace. Each needs a
    /// choice before the import can run.
    pub unmapped_users: Vec<UnmappedUser>,
    /// Invite links for accounts the import created, to share with them.
    pub invites: Vec<CreatedInvite>,
    /// True when the data was written (never for a preview, nor when something blocked it).
    pub applied: bool,
    /// Things that were imported with a change (an unknown assignee ignored, ...).
    pub warnings: Vec<String>,
}

pub fn day_text(d: Option<jiff::civil::DateTime>) -> Option<String> {
    d.map(|d| time::timestamp(d).to_string())
}

/// What to export.
pub struct Parts {
    pub labels: bool,
    pub members: bool,
    pub projects: bool,
    pub wiki: bool,
}

mod export;
mod import;
/// Build the export document for the chosen parts. `project_ids` narrows the projects (all when empty).
mod tasks_csv;

pub use export::build_export;
pub use import::apply;
pub use tasks_csv::{parse_tasks_csv, tasks_csv};
