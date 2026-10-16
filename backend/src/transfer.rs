//! Moving data in and out: a JSON document of chosen parts of a workspace, and a CSV of tasks that holds
//! everything needed to set up projects and tasks. Both formats are Woodmouse's own; both are imported
//! through one plan (`Export`), which can be previewed (`dry_run`) before anything is written. An import is
//! not atomic: what was created before an error stays.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::audit::{self, Entry};
use crate::blocks::BlockData;
use crate::error::ApiError;
use crate::models::{
    Comment, Label, List, Priority, Project, Role, Sprint, Task, TaskAssignee, TaskLabel, User,
    WikiPage, Workspace, WorkspaceMember,
};
use crate::routes::tasks::{CreateTaskRequest, create_task_tx};
use crate::{blocks, pages, revisions, time};

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

/// Build the export document for the chosen parts. `project_ids` narrows the projects (all when empty).
pub async fn build_export(
    db: &mut dyn toasty::Executor,
    workspace: &Workspace,
    parts: &Parts,
    project_ids: &[uuid::Uuid],
) -> Result<Export, ApiError> {
    let users: HashMap<uuid::Uuid, User> = User::all()
        .exec(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();
    let email_of = |id: &uuid::Uuid| users.get(id).map(|u| u.email.clone()).unwrap_or_default();
    let labels: Vec<Label> = Label::filter_by_workspace_id(workspace.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .collect();
    let mut out = Export {
        version: VERSION,
        exported_at: time::timestamp(time::now()).to_string(),
        workspace: Some(ExportWorkspace {
            name: workspace.name.clone(),
            description: workspace.description.clone(),
        }),
        ..Default::default()
    };
    if parts.labels {
        out.labels = Some(
            labels
                .iter()
                .map(|l| ExportLabel {
                    name: l.name.clone(),
                    color: Some(l.color.clone()),
                })
                .collect(),
        );
    }
    if parts.members {
        let mut members = vec![];
        for m in WorkspaceMember::all()
            .filter(WorkspaceMember::fields().workspace_id().eq(workspace.id))
            .exec(db)
            .await?
        {
            if let Some(u) = users.get(&m.user_id) {
                members.push(ExportMember {
                    email: u.email.clone(),
                    name: u.name.clone(),
                    role: m.role,
                });
            }
        }
        members.sort_by(|a, b| a.email.cmp(&b.email));
        out.members = Some(members);
    }
    if parts.projects {
        let mut projects: Vec<Project> = Project::filter_by_workspace_id(workspace.id)
            .exec(db)
            .await?
            .into_iter()
            .filter(|p| {
                p.deleted_at.is_none() && (project_ids.is_empty() || project_ids.contains(&p.id))
            })
            .collect();
        projects.sort_by_key(|p| p.name.to_lowercase());
        let mut exported = vec![];
        for project in projects {
            exported.push(export_project(db, &project, &labels, &email_of).await?);
        }
        out.projects = Some(exported);
    }
    if parts.wiki {
        let mut pages: Vec<WikiPage> = WikiPage::filter_by_workspace_id(workspace.id)
            .exec(db)
            .await?
            .into_iter()
            .filter(|p| p.deleted_at.is_none())
            .collect();
        pages.sort_by_key(|p| (p.position, p.id));
        // Number them parents-first.
        let mut order: Vec<&WikiPage> = vec![];
        let mut queue: Vec<Option<uuid::Uuid>> = vec![None];
        while let Some(parent) = queue.pop() {
            for p in pages.iter().filter(|p| p.parent_id == parent) {
                order.push(p);
                queue.push(Some(p.id));
            }
        }
        // Pages whose parent is gone are treated as top level.
        for p in &pages {
            if !order.iter().any(|o| o.id == p.id) {
                order.push(p);
            }
        }
        let numbers: HashMap<uuid::Uuid, u32> = order
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id, i as u32 + 1))
            .collect();
        out.wiki = Some(
            order
                .iter()
                .map(|p| ExportPage {
                    id: numbers[&p.id],
                    parent: p.parent_id.and_then(|id| numbers.get(&id).copied()),
                    title: p.title.clone(),
                    blocks: pages::blocks_of(p).unwrap_or_default(),
                })
                .collect(),
        );
    }
    Ok(out)
}

async fn export_project(
    db: &mut dyn toasty::Executor,
    project: &Project,
    labels: &[Label],
    email_of: &(dyn Fn(&uuid::Uuid) -> String + Sync),
) -> Result<ExportProject, ApiError> {
    let mut lists: Vec<List> = List::filter_by_project_id(project.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .collect();
    lists.sort_by_key(|l| (l.position, l.id));
    let sprints: Vec<Sprint> = Sprint::filter_by_project_id(project.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|s| s.deleted_at.is_none())
        .collect();
    let mut tasks: Vec<Task> = Task::filter_by_project_id(project.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at.is_none())
        .collect();
    tasks.sort_by_key(|t| (t.position, t.id));
    // Parents before subtasks, numbered from 1.
    let mut order: Vec<&Task> = vec![];
    let mut queue: Vec<Option<uuid::Uuid>> = vec![None];
    while let Some(parent) = queue.pop() {
        for t in tasks.iter().filter(|t| t.parent_id == parent) {
            order.push(t);
            queue.push(Some(t.id));
        }
    }
    let numbers: HashMap<uuid::Uuid, u32> = order
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i as u32 + 1))
        .collect();
    let assignees = TaskAssignee::all().exec(db).await?;
    let task_labels = TaskLabel::all().exec(db).await?;
    let comments = Comment::all().exec(db).await?;
    let mut exported = vec![];
    for t in order {
        let mut cs: Vec<&Comment> = comments
            .iter()
            .filter(|c| c.task_id == t.id && c.deleted_at.is_none())
            .collect();
        cs.sort_by_key(|c| (c.created_at, c.id));
        exported.push(ExportTask {
            id: numbers[&t.id],
            parent: t.parent_id.and_then(|p| numbers.get(&p).copied()),
            list: t
                .list_id
                .and_then(|id| lists.iter().find(|l| l.id == id))
                .map(|l| l.name.clone()),
            sprint: t
                .sprint_id
                .and_then(|id| sprints.iter().find(|s| s.id == id))
                .map(|s| s.name.clone()),
            title: t.title.clone(),
            description: t.description.clone(),
            priority: Some(
                serde_json::to_value(t.priority)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "medium".into()),
            ),
            start_date: day_text(t.start_date),
            due_date: day_text(t.due_date),
            estimate_minutes: t.estimate_minutes,
            spent_minutes: t.spent_minutes,
            completed: day_text(t.completed_at),
            labels: task_labels
                .iter()
                .filter(|l| l.task_id == t.id)
                .filter_map(|l| labels.iter().find(|x| x.id == l.label_id))
                .map(|l| l.name.clone())
                .collect(),
            assignees: assignees
                .iter()
                .filter(|a| a.task_id == t.id)
                .map(|a| email_of(&a.user_id))
                .collect(),
            comments: cs
                .iter()
                .map(|c| ExportComment {
                    author: email_of(&c.author_id),
                    body: c.body.clone(),
                    created_at: day_text(Some(c.created_at)),
                })
                .collect(),
        });
    }
    Ok(ExportProject {
        name: project.name.clone(),
        description: project.description.clone(),
        lists: lists.iter().map(|l| l.name.clone()).collect(),
        sprints: sprints
            .iter()
            .map(|s| ExportSprint {
                name: s.name.clone(),
                goal: s.goal.clone(),
                start_date: day_text(Some(s.start_date)),
                end_date: day_text(Some(s.end_date)),
                completed_at: day_text(s.completed_at),
            })
            .collect(),
        tasks: exported,
    })
}

// ----- CSV -----

const CSV_COLUMNS: [&str; 19] = [
    "id",
    "parent",
    "project",
    "project_description",
    "list",
    "sprint",
    "sprint_start",
    "sprint_end",
    "sprint_goal",
    "title",
    "description",
    "priority",
    "start_date",
    "due_date",
    "estimate_minutes",
    "spent_minutes",
    "completed",
    "labels",
    "assignees",
];

/// The tasks of the given projects as CSV: one row per task, with the project, list and sprint on every
/// row, so the file alone can set everything up again. Labels and assignee emails are `;` separated.
pub fn tasks_csv(projects: &[ExportProject]) -> Result<String, ApiError> {
    let mut w = csv::Writer::from_writer(vec![]);
    let fail = |e: &dyn std::fmt::Display| {
        tracing::error!(%e, "writing CSV");
        ApiError::internal("could not write the CSV")
    };
    w.write_record(CSV_COLUMNS).map_err(|e| fail(&e))?;
    for p in projects {
        for t in &p.tasks {
            let sprint = t
                .sprint
                .as_ref()
                .and_then(|name| p.sprints.iter().find(|s| &s.name == name));
            w.write_record([
                t.id.to_string(),
                t.parent.map(|p| p.to_string()).unwrap_or_default(),
                p.name.clone(),
                p.description.clone(),
                t.list.clone().unwrap_or_default(),
                t.sprint.clone().unwrap_or_default(),
                sprint
                    .and_then(|s| s.start_date.clone())
                    .unwrap_or_default(),
                sprint.and_then(|s| s.end_date.clone()).unwrap_or_default(),
                sprint.map(|s| s.goal.clone()).unwrap_or_default(),
                t.title.clone(),
                t.description.clone(),
                t.priority.clone().unwrap_or_default(),
                t.start_date.clone().unwrap_or_default(),
                t.due_date.clone().unwrap_or_default(),
                t.estimate_minutes
                    .map(|m| m.to_string())
                    .unwrap_or_default(),
                t.spent_minutes.map(|m| m.to_string()).unwrap_or_default(),
                t.completed.clone().unwrap_or_default(),
                t.labels.join(";"),
                t.assignees.join(";"),
            ])
            .map_err(|e| fail(&e))?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| fail(&e))?).map_err(|e| fail(&e))
}

/// Read a tasks CSV into projects. `default_project` names the project for rows without one. Rows that
/// cannot be read are reported, not fatal.
pub fn parse_tasks_csv(
    text: &str,
    default_project: Option<&str>,
    report: &mut ImportReport,
) -> Result<Vec<ExportProject>, ApiError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ApiError::bad_request(format!("the CSV has no header row: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    if !headers.iter().any(|h| h == "title") {
        return Err(ApiError::bad_request("the CSV needs a `title` column"));
    }
    let col = |name: &str| headers.iter().position(|h| h == name);
    let mut projects: Vec<ExportProject> = vec![];
    for (i, record) in reader.records().enumerate() {
        let row = i + 2; // the header is row 1
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                report.problems.push(format!("row {row}: {e}"));
                continue;
            }
        };
        let get = |name: &str| -> String {
            col(name)
                .and_then(|c| record.get(c))
                .map(str::trim)
                .unwrap_or_default()
                .to_string()
        };
        let some = |name: &str| Some(get(name)).filter(|v| !v.is_empty());
        if record.iter().all(|f| f.trim().is_empty()) {
            continue; // a blank row is not a problem
        }
        let title = get("title");
        if title.is_empty() {
            report.problems.push(format!("row {row}: no title"));
            continue;
        }
        let project_name = some("project")
            .or_else(|| default_project.map(str::to_string))
            .filter(|p| !p.is_empty());
        let Some(project_name) = project_name else {
            report.problems.push(format!(
                "row {row}: no project (add a `project` column or choose one)"
            ));
            continue;
        };
        let number = |name: &str| -> Result<Option<i64>, String> {
            match some(name) {
                None => Ok(None),
                Some(v) => v
                    .parse::<i64>()
                    .map(Some)
                    .map_err(|_| format!("{name} `{v}` is not a number")),
            }
        };
        let (estimate, spent) = match (number("estimate_minutes"), number("spent_minutes")) {
            (Ok(e), Ok(s)) => (e, s),
            (Err(e), _) | (_, Err(e)) => {
                report.problems.push(format!("row {row}: {e}"));
                continue;
            }
        };
        let idx = match projects.iter().position(|p| p.name == project_name) {
            Some(i) => i,
            None => {
                projects.push(ExportProject {
                    name: project_name,
                    description: get("project_description"),
                    ..Default::default()
                });
                projects.len() - 1
            }
        };
        let project = &mut projects[idx];
        if project.description.is_empty() {
            project.description = get("project_description");
        }
        let list = some("list");
        if let Some(list) = &list
            && !project.lists.contains(list)
        {
            project.lists.push(list.clone());
        }
        let sprint = some("sprint");
        if let Some(name) = &sprint
            && !project.sprints.iter().any(|s| &s.name == name)
        {
            project.sprints.push(ExportSprint {
                name: name.clone(),
                goal: get("sprint_goal"),
                start_date: some("sprint_start"),
                end_date: some("sprint_end"),
                completed_at: None,
            });
        }
        let id = some("id")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(project.tasks.len() as u32 + 1);
        let split = |v: String| -> Vec<String> {
            v.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        project.tasks.push(ExportTask {
            id,
            parent: some("parent").and_then(|v| v.parse().ok()),
            list,
            sprint,
            title,
            description: get("description"),
            priority: some("priority"),
            start_date: some("start_date"),
            due_date: some("due_date"),
            estimate_minutes: estimate,
            spent_minutes: spent,
            completed: some("completed")
                .filter(|v| !["false", "no", "0"].contains(&v.to_lowercase().as_str())),
            labels: split(get("labels")),
            assignees: split(get("assignees")),
            comments: vec![],
        });
    }
    Ok(projects)
}

// ----- importing -----

/// A date or date-time as a UTC date-time (`YYYY-MM-DD` means that day at midnight).
fn parse_when(text: &str) -> Option<jiff::civil::DateTime> {
    let t = text.trim();
    if let Ok(ts) = t.parse::<jiff::Timestamp>() {
        return Some(time::from_timestamp(ts));
    }
    t.parse::<jiff::civil::Date>().ok().map(time::day)
}

fn parse_priority(text: &str) -> Option<Priority> {
    match text.trim().to_lowercase().as_str() {
        "low" => Some(Priority::Low),
        "medium" | "normal" => Some(Priority::Medium),
        "high" => Some(Priority::High),
        "urgent" | "critical" => Some(Priority::Urgent),
        _ => None,
    }
}

/// Import `data` into the workspace: everything, or nothing. A preview (`dry_run`) only reports. A real
/// import first previews; if the file has problems, or people who still need a choice, it is refused with
/// that report (`applied` false). Otherwise everything is written in one transaction.
pub async fn apply(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    data: Export,
    options: &ImportOptions,
) -> Result<ImportReport, ApiError> {
    let preview = {
        let mut report = ImportReport {
            dry_run: true,
            ..Default::default()
        };
        run(
            &mut *db,
            workspace_id,
            actor,
            &data,
            options,
            true,
            &mut report,
        )
        .await?;
        report
    };
    if options.dry_run {
        return Ok(preview);
    }
    if !preview.problems.is_empty() || !preview.unmapped_users.is_empty() {
        return Ok(ImportReport {
            dry_run: false,
            applied: false,
            ..preview
        });
    }
    let mut tx = db.transaction().await?;
    let mut report = ImportReport::default();
    // Any error rolls everything back (the transaction is dropped without a commit).
    run(
        &mut tx,
        workspace_id,
        actor,
        &data,
        options,
        false,
        &mut report,
    )
    .await?;
    tx.commit().await?;
    report.applied = true;
    Ok(report)
}

/// Resolve the people the file names to users of this workspace, creating or adding some when asked to.
/// `None` means "leave out". Anyone without a choice is reported as unmapped.
async fn resolve_users(
    ex: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    data: &Export,
    options: &ImportOptions,
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<HashMap<String, Option<uuid::Uuid>>, ApiError> {
    // Who the file names, and how often.
    let mut named: Vec<(String, usize)> = vec![];
    let mut count = |email: &str| {
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            return;
        }
        match named.iter_mut().find(|(e, _)| *e == email) {
            Some((_, n)) => *n += 1,
            None => named.push((email, 1)),
        }
    };
    for t in data.projects.iter().flatten().flat_map(|p| &p.tasks) {
        t.assignees.iter().for_each(|e| count(e));
        t.comments.iter().for_each(|c| count(&c.author));
    }

    let users: HashMap<uuid::Uuid, String> = User::all()
        .exec(&mut *ex)
        .await?
        .into_iter()
        .map(|u| (u.id, u.email.to_lowercase()))
        .collect();
    let members: HashMap<String, uuid::Uuid> = WorkspaceMember::all()
        .filter(WorkspaceMember::fields().workspace_id().eq(workspace_id))
        .exec(&mut *ex)
        .await?
        .into_iter()
        .filter_map(|m| users.get(&m.user_id).map(|e| (e.clone(), m.user_id)))
        .collect();
    let by_email: HashMap<&String, uuid::Uuid> = users.iter().map(|(id, e)| (e, *id)).collect();

    let mut out: HashMap<String, Option<uuid::Uuid>> = HashMap::new();
    for (email, references) in named {
        if let Some(id) = members.get(&email) {
            out.insert(email, Some(*id));
            continue;
        }
        match options.user_map.get(&email) {
            None => report
                .unmapped_users
                .push(UnmappedUser { email, references }),
            Some(UserChoice::Skip) => {
                out.insert(email, None);
            }
            Some(UserChoice::Map { user_id }) => {
                if members.values().any(|m| m == user_id) {
                    out.insert(email, Some(*user_id));
                } else {
                    report.problems.push(format!(
                        "{email}: the person chosen is not a member of this workspace"
                    ));
                }
            }
            Some(UserChoice::Create { role }) => {
                if !options.can_create_users {
                    report.problems.push(format!(
                        "{email}: only a workspace admin can create people or add them to the workspace"
                    ));
                    continue;
                }
                if dry_run {
                    out.insert(email, Some(uuid::Uuid::new_v4()));
                    continue;
                }
                // An existing account is added to the workspace; otherwise one is created with an invite.
                let (user_id, new_account) = match by_email.get(&email) {
                    Some(id) => (*id, false),
                    None => {
                        let created = toasty::create!(User {
                            email: email.clone(),
                            name: String::new(),
                            password_hash: String::new(),
                            super_admin: false,
                            active: true,
                        })
                        .exec(&mut *ex)
                        .await?;
                        (created.id, true)
                    }
                };
                toasty::create!(WorkspaceMember {
                    user_id,
                    workspace_id,
                    role: *role,
                })
                .exec(&mut *ex)
                .await?;
                audit::record(
                    &mut *ex,
                    Entry::feed(workspace_id, actor, "member_added")
                        .target("user", user_id)
                        .data(serde_json::json!({ "role": role })),
                )
                .await?;
                if new_account {
                    let invite_token =
                        crate::routes::members::issue_invite(&mut *ex, user_id).await?;
                    report.invites.push(CreatedInvite {
                        email: email.clone(),
                        invite_token,
                    });
                }
                out.insert(email, Some(user_id));
            }
        }
    }
    Ok(out)
}

/// One pass over the data: with `dry_run` it only fills in the report, otherwise it writes through `ex`
/// (which the caller makes a transaction).
async fn run(
    ex: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    data: &Export,
    options: &ImportOptions,
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<(), ApiError> {
    let users = resolve_users(
        &mut *ex,
        workspace_id,
        actor,
        data,
        options,
        dry_run,
        report,
    )
    .await?;
    let mut labels: HashMap<String, uuid::Uuid> = Label::filter_by_workspace_id(workspace_id)
        .exec(&mut *ex)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .map(|l| (l.name.to_lowercase(), l.id))
        .collect();
    if data.members.as_ref().is_some_and(|m| !m.is_empty()) {
        report.warnings.push(
            "the member list in the file is for reference; people are matched by email when tasks and comments name them"
                .into(),
        );
    }

    // Labels: from the file's list, and any a task names.
    let mut wanted: Vec<(String, Option<String>)> = data
        .labels
        .iter()
        .flatten()
        .map(|l| (l.name.clone(), l.color.clone()))
        .collect();
    for t in data.projects.iter().flatten().flat_map(|p| &p.tasks) {
        for l in &t.labels {
            wanted.push((l.clone(), None));
        }
    }
    for (name, color) in wanted {
        let name = name.trim().to_string();
        if name.is_empty() || labels.contains_key(&name.to_lowercase()) {
            continue;
        }
        let id = if dry_run {
            uuid::Uuid::new_v4()
        } else {
            toasty::create!(Label {
                workspace_id,
                name: name.clone(),
                color: color.unwrap_or_else(|| "#71717a".to_string()),
            })
            .exec(&mut *ex)
            .await?
            .id
        };
        labels.insert(name.to_lowercase(), id);
        report.labels_created += 1;
    }

    let existing_projects: Vec<Project> = Project::filter_by_workspace_id(workspace_id)
        .exec(&mut *ex)
        .await?
        .into_iter()
        .filter(|p| p.deleted_at.is_none())
        .collect();
    for project in data.projects.iter().flatten() {
        let name = project.name.trim().to_string();
        if name.is_empty() {
            report.problems.push("a project without a name".into());
            continue;
        }
        let (project_id, existing) = match existing_projects
            .iter()
            .find(|p| p.name.to_lowercase() == name.to_lowercase())
        {
            Some(p) => (p.id, true),
            None => {
                report.projects_created += 1;
                let id = if dry_run {
                    uuid::Uuid::new_v4()
                } else {
                    toasty::create!(Project {
                        workspace_id,
                        name: name.clone(),
                        description: project.description.clone(),
                    })
                    .exec(&mut *ex)
                    .await?
                    .id
                };
                (id, false)
            }
        };
        import_project(
            ex,
            workspace_id,
            actor,
            project_id,
            existing,
            &name,
            project,
            &labels,
            &users,
            dry_run,
            report,
        )
        .await?;
    }

    if let Some(wiki) = &data.wiki {
        import_wiki(ex, workspace_id, actor, wiki, dry_run, report).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn import_project(
    ex: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    project_id: uuid::Uuid,
    existing: bool,
    project_name: &str,
    project: &ExportProject,
    labels: &HashMap<String, uuid::Uuid>,
    users: &HashMap<String, Option<uuid::Uuid>>,
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<(), ApiError> {
    let mut lists: HashMap<String, uuid::Uuid> = if existing {
        List::filter_by_project_id(project_id)
            .exec(&mut *ex)
            .await?
            .into_iter()
            .filter(|l| l.deleted_at.is_none())
            .map(|l| (l.name.to_lowercase(), l.id))
            .collect()
    } else {
        HashMap::new()
    };
    let mut sprints: HashMap<String, uuid::Uuid> = if existing {
        Sprint::filter_by_project_id(project_id)
            .exec(&mut *ex)
            .await?
            .into_iter()
            .filter(|s| s.deleted_at.is_none() && s.completed_at.is_none())
            .map(|s| (s.name.to_lowercase(), s.id))
            .collect()
    } else {
        HashMap::new()
    };
    let mut next_position = crate::ordering::GAP * lists.len() as i64;
    // Lists named by the project or by its tasks.
    let mut list_names: Vec<String> = project.lists.clone();
    list_names.extend(project.tasks.iter().filter_map(|t| t.list.clone()));
    for name in list_names {
        let name = name.trim().to_string();
        if !name.is_empty() && !lists.contains_key(&name.to_lowercase()) {
            next_position += crate::ordering::GAP;
            let id = if dry_run {
                uuid::Uuid::new_v4()
            } else {
                toasty::create!(List {
                    project_id,
                    name: name.clone(),
                    position: next_position,
                })
                .exec(&mut *ex)
                .await?
                .id
            };
            lists.insert(name.to_lowercase(), id);
            report.lists_created += 1;
        }
    }
    // Sprints named by the project or by its tasks (without dates: two weeks from today).
    let mut sprint_specs: Vec<ExportSprint> = project.sprints.clone();
    for t in &project.tasks {
        if let Some(name) = &t.sprint
            && !sprint_specs.iter().any(|s| &s.name == name)
        {
            sprint_specs.push(ExportSprint {
                name: name.clone(),
                goal: String::new(),
                start_date: None,
                end_date: None,
                completed_at: None,
            });
        }
    }
    for spec in sprint_specs {
        let name = spec.name.trim().to_string();
        if name.is_empty() || sprints.contains_key(&name.to_lowercase()) {
            continue;
        }
        let today = time::to_day(time::now());
        let start = spec
            .start_date
            .as_deref()
            .and_then(parse_when)
            .map(time::to_day);
        let end = spec
            .end_date
            .as_deref()
            .and_then(parse_when)
            .map(time::to_day);
        let (start, end) = match (start, end) {
            (Some(s), Some(e)) if e >= s => (s, e),
            (Some(s), _) => (s, time::add_seconds(s, 13 * 86_400)),
            _ => {
                report.warnings.push(format!(
                    "{project_name}: sprint \"{name}\" has no usable dates; it runs two weeks from today"
                ));
                (today, time::add_seconds(today, 13 * 86_400))
            }
        };
        let completed = spec.completed_at.as_deref().and_then(parse_when);
        let id = if dry_run {
            uuid::Uuid::new_v4()
        } else {
            toasty::create!(Sprint {
                project_id,
                name: name.clone(),
                goal: spec.goal.clone(),
                start_date: start,
                end_date: end,
                completed_at: completed,
            })
            .exec(&mut *ex)
            .await?
            .id
        };
        // Tasks can only join open sprints; a completed one is kept as a record only.
        if completed.is_none() {
            sprints.insert(name.to_lowercase(), id);
        }
        report.sprints_created += 1;
    }

    // Tasks, parents first. `created` maps the file's task numbers to the new ids.
    let mut created: HashMap<u32, (uuid::Uuid, Option<uuid::Uuid>)> = HashMap::new();
    for t in &project.tasks {
        let title = t.title.trim().to_string();
        if title.is_empty() {
            report
                .problems
                .push(format!("{project_name}: a task without a title"));
            continue;
        }
        let parent = match t.parent {
            None => None,
            Some(n) => match created.get(&n) {
                Some(found) => Some(*found),
                None => {
                    report.problems.push(format!(
                        "{project_name}: \"{title}\" names a parent (id {n}) that does not come before it"
                    ));
                    continue;
                }
            },
        };
        let list_id = match &parent {
            Some((_, parent_list)) => *parent_list,
            None => t
                .list
                .as_deref()
                .and_then(|n| lists.get(&n.trim().to_lowercase()).copied()),
        };
        let assignee_ids: Vec<uuid::Uuid> = t
            .assignees
            .iter()
            .filter_map(|email| users.get(&email.trim().to_lowercase()).copied().flatten())
            .collect();
        let label_ids: Vec<uuid::Uuid> = t
            .labels
            .iter()
            .filter_map(|l| labels.get(&l.trim().to_lowercase()).copied())
            .collect();
        let mut problem = |what: String| {
            report
                .problems
                .push(format!("{project_name}: \"{title}\": {what}"))
        };
        let date =
            |v: &Option<String>, what: &str, problem: &mut dyn FnMut(String)| match v.as_deref() {
                None => None,
                Some(text) => {
                    let parsed = parse_when(text);
                    if parsed.is_none() {
                        problem(format!("{what} `{text}` is not a date"));
                    }
                    parsed
                }
            };
        let start_date = date(&t.start_date, "start date", &mut problem);
        let due_date = date(&t.due_date, "due date", &mut problem);
        let priority = match t.priority.as_deref() {
            None => None,
            Some(text) => {
                let parsed = parse_priority(text);
                if parsed.is_none() {
                    problem(format!(
                        "priority `{text}` is not low, medium, high or urgent"
                    ));
                }
                parsed
            }
        };
        for (what, value) in [
            ("estimate", t.estimate_minutes),
            ("time spent", t.spent_minutes),
        ] {
            if value.is_some_and(|v| !(0..=1_000_000).contains(&v)) {
                problem(format!("{what} is out of range"));
            }
        }
        let sprint_id = t
            .sprint
            .as_deref()
            .and_then(|n| sprints.get(&n.trim().to_lowercase()).copied());
        let completed_at = t
            .completed
            .as_deref()
            .map(|c| parse_when(c).unwrap_or_else(time::now));
        if dry_run {
            created.insert(t.id, (uuid::Uuid::new_v4(), list_id));
            report.tasks_created += 1;
            report.comments_created += t
                .comments
                .iter()
                .filter(|c| !c.body.trim().is_empty())
                .count();
            continue;
        }
        let task = create_task_tx(
            &mut *ex,
            workspace_id,
            project_id,
            list_id,
            parent.map(|(id, _)| id),
            actor,
            CreateTaskRequest {
                title: title.clone(),
                description: t.description.clone(),
                priority,
                start_date,
                due_date,
                sprint_id,
                estimate_minutes: t.estimate_minutes,
                spent_minutes: t.spent_minutes,
                assignee_ids,
                label_ids,
            },
        )
        .await?;
        created.insert(t.id, (task.id, task.list_id));
        report.tasks_created += 1;
        if let Some(done) = completed_at {
            Task::filter_by_id(task.id)
                .update()
                .completed_at(Some(done))
                .exec(&mut *ex)
                .await?;
        }
        for c in &t.comments {
            let body = c.body.trim().to_string();
            if body.is_empty() {
                continue;
            }
            let author = users
                .get(&c.author.trim().to_lowercase())
                .copied()
                .flatten()
                .unwrap_or(actor);
            let at = c
                .created_at
                .as_deref()
                .and_then(parse_when)
                .unwrap_or_else(time::now);
            let comment = toasty::create!(Comment {
                task_id: task.id,
                author_id: author,
                body: body.clone(),
                created_at: at,
            })
            .exec(&mut *ex)
            .await?;
            revisions::record_comment(&mut *ex, comment.id, author, &body).await?;
            report.comments_created += 1;
        }
    }
    Ok(())
}

async fn import_wiki(
    ex: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    wiki: &[ExportPage],
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<(), ApiError> {
    let mut created: HashMap<u32, uuid::Uuid> = HashMap::new();
    let existing: Vec<WikiPage> = WikiPage::filter_by_workspace_id(workspace_id)
        .exec(&mut *ex)
        .await?
        .into_iter()
        .filter(|p| p.deleted_at.is_none())
        .collect();
    let mut positions: HashMap<Option<uuid::Uuid>, i64> = HashMap::new();
    for p in &existing {
        let entry = positions.entry(p.parent_id).or_insert(0);
        *entry = (*entry).max(p.position);
    }
    for page in wiki {
        let title: String = page.title.trim().chars().take(200).collect();
        let title = if title.is_empty() {
            "Untitled".to_string()
        } else {
            title
        };
        let parent = match page.parent {
            None => None,
            Some(n) => match created.get(&n) {
                Some(id) => Some(*id),
                None => {
                    report.problems.push(format!(
                        "page \"{title}\" names a parent (id {n}) that does not come before it"
                    ));
                    continue;
                }
            },
        };
        if blocks::validate_blocks(&page.blocks).is_err() {
            report
                .problems
                .push(format!("page \"{title}\" has content that is not valid"));
            continue;
        }
        if dry_run {
            created.insert(page.id, uuid::Uuid::new_v4());
            report.pages_created += 1;
            continue;
        }
        let position = positions.entry(parent).or_insert(0);
        *position += crate::ordering::GAP;
        let now = time::now();
        let mut row = toasty::create!(WikiPage {
            workspace_id,
            parent_id: parent,
            title: title.clone(),
            position: *position,
            content: blocks::encode(&[]),
            created_by: actor,
            created_at: now,
            updated_by: actor,
            updated_at: now,
            version: 0,
        })
        .exec(&mut *ex)
        .await?;
        revisions::record_page(
            &mut *ex,
            row.id,
            actor,
            &revisions::PageContent {
                title: title.clone(),
                blocks: vec![],
            },
            true,
        )
        .await?;
        pages::save_content(&mut *ex, &mut row, actor, &page.blocks, true).await?;
        created.insert(page.id, row.id);
        report.pages_created += 1;
    }
    Ok(())
}
