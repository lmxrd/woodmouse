//! Importing a document, as one plan that can be previewed first.

use crate::db::Db;
use crate::db::prelude::*;
use entity::{comments, tasks};
use entity::{labels, lists, projects, sprints, users, wiki_pages, workspace_members};
use std::collections::HashMap;

use crate::audit::{self, Entry};
use crate::error::ApiError;
use crate::routes::tasks::{CreateTaskRequest, create_task_tx};
use crate::{blocks, pages, revisions, time};
use entity::{Priority, Project, WikiPage};

use super::*;

/// A date or date-time as a UTC date-time (`YYYY-MM-DD` means that day at midnight).
fn parse_when(text: &str) -> Option<crate::time::DateTime> {
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
    db: &impl Db,
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
        run(db, workspace_id, actor, &data, options, true, &mut report).await?;
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
    let tx = db.begin().await?;
    let mut report = ImportReport::default();
    // Any error rolls everything back (the transaction is dropped without a commit).
    run(&tx, workspace_id, actor, &data, options, false, &mut report).await?;
    tx.commit().await?;
    report.applied = true;
    Ok(report)
}

/// Resolve the people the file names to users of this workspace, creating or adding some when asked to.
/// `None` means "leave out". Anyone without a choice is reported as unmapped.
async fn resolve_users(
    ex: &impl Db,
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

    let users: HashMap<uuid::Uuid, String> = users::Entity::find()
        .all(ex)
        .await?
        .into_iter()
        .map(|u| (u.id, u.email.to_lowercase()))
        .collect();
    let members: HashMap<String, uuid::Uuid> = workspace_members::Entity::find()
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .all(ex)
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
                        let created = users::ActiveModel {
                            id: Set(uuid::Uuid::now_v7()),
                            email: Set(email.clone()),
                            name: Set(String::new()),
                            password_hash: Set(String::new()),
                            super_admin: Set(false),
                            active: Set(true),
                            ..Default::default()
                        }
                        .insert(ex)
                        .await?;
                        (created.id, true)
                    }
                };
                workspace_members::ActiveModel {
                    user_id: Set(user_id),
                    workspace_id: Set(workspace_id),
                    role: Set(*role),
                    ..Default::default()
                }
                .insert(ex)
                .await?;
                audit::record(
                    ex,
                    Entry::feed(workspace_id, actor, "member_added")
                        .target("user", user_id)
                        .data(serde_json::json!({ "role": role })),
                )
                .await?;
                if new_account {
                    let invite_token = crate::routes::members::issue_invite(ex, user_id).await?;
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
    ex: &impl Db,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    data: &Export,
    options: &ImportOptions,
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<(), ApiError> {
    let users = resolve_users(ex, workspace_id, actor, data, options, dry_run, report).await?;
    let mut labels: HashMap<String, uuid::Uuid> = labels::Entity::find()
        .filter(labels::Column::WorkspaceId.eq(workspace_id))
        .filter(labels::Column::DeletedAt.is_null())
        .all(ex)
        .await?
        .into_iter()
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
            labels::ActiveModel {
                id: Set(uuid::Uuid::now_v7()),
                workspace_id: Set(workspace_id),
                name: Set(name.clone()),
                color: Set(color.unwrap_or_else(|| "#71717a".to_string())),
                ..Default::default()
            }
            .insert(ex)
            .await?
            .id
        };
        labels.insert(name.to_lowercase(), id);
        report.labels_created += 1;
    }

    let existing_projects: Vec<Project> = projects::Entity::find()
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::DeletedAt.is_null())
        .all(ex)
        .await?
        .into_iter()
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
                    projects::ActiveModel {
                        id: Set(uuid::Uuid::now_v7()),
                        workspace_id: Set(workspace_id),
                        name: Set(name.clone()),
                        description: Set(project.description.clone()),
                        ..Default::default()
                    }
                    .insert(ex)
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
    ex: &impl Db,
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
        lists::Entity::find()
            .filter(lists::Column::ProjectId.eq(project_id))
            .filter(lists::Column::DeletedAt.is_null())
            .all(ex)
            .await?
            .into_iter()
            .map(|l| (l.name.to_lowercase(), l.id))
            .collect()
    } else {
        HashMap::new()
    };
    let mut sprints: HashMap<String, uuid::Uuid> = if existing {
        sprints::Entity::find()
            .filter(sprints::Column::ProjectId.eq(project_id))
            .filter(sprints::Column::DeletedAt.is_null())
            .filter(sprints::Column::CompletedAt.is_null())
            .all(ex)
            .await?
            .into_iter()
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
                lists::ActiveModel {
                    id: Set(uuid::Uuid::now_v7()),
                    project_id: Set(project_id),
                    name: Set(name.clone()),
                    position: Set(next_position),
                    ..Default::default()
                }
                .insert(ex)
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
            sprints::ActiveModel {
                id: Set(uuid::Uuid::now_v7()),
                project_id: Set(project_id),
                name: Set(name.clone()),
                goal: Set(spec.goal.clone()),
                start_date: Set(start),
                end_date: Set(end),
                completed_at: Set(completed),
                ..Default::default()
            }
            .insert(ex)
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
            ex,
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
            tasks::ActiveModel {
                id: Unchanged(task.id),
                completed_at: Set(Some(done)),
                ..Default::default()
            }
            .update(ex)
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
            let comment = comments::ActiveModel {
                id: Set(uuid::Uuid::now_v7()),
                task_id: Set(task.id),
                author_id: Set(author),
                body: Set(body.clone()),
                created_at: Set(at),
                ..Default::default()
            }
            .insert(ex)
            .await?;
            revisions::record_comment(ex, comment.id, author, &body).await?;
            report.comments_created += 1;
        }
    }
    Ok(())
}

async fn import_wiki(
    ex: &impl Db,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    wiki: &[ExportPage],
    dry_run: bool,
    report: &mut ImportReport,
) -> Result<(), ApiError> {
    let mut created: HashMap<u32, uuid::Uuid> = HashMap::new();
    let existing: Vec<WikiPage> = wiki_pages::Entity::find()
        .filter(wiki_pages::Column::WorkspaceId.eq(workspace_id))
        .filter(wiki_pages::Column::DeletedAt.is_null())
        .all(ex)
        .await?
        .into_iter()
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
        let mut row = wiki_pages::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            workspace_id: Set(workspace_id),
            parent_id: Set(parent),
            title: Set(title.clone()),
            position: Set(*position),
            content: Set(blocks::encode(&[])),
            created_by: Set(actor),
            created_at: Set(now),
            updated_by: Set(actor),
            updated_at: Set(now),
            version: Set(0),
            ..Default::default()
        }
        .insert(ex)
        .await?;
        revisions::record_page(
            ex,
            row.id,
            actor,
            &revisions::PageContent {
                title: title.clone(),
                blocks: vec![],
            },
            true,
        )
        .await?;
        pages::save_content(ex, &mut row, actor, &page.blocks, true).await?;
        created.insert(page.id, row.id);
        report.pages_created += 1;
    }
    Ok(())
}
