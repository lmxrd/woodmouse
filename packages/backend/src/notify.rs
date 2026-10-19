//! Notifications: tell people about what happens to the things they care about (tasks assigned to them,
//! comments on their tasks, mentions, tasks and pages they watch, deadlines). A notification is written in
//! the transaction of the change it is about; emailing is a separate sweep (`jobs`).

use serde_json::{Value, json};

use crate::access;
use crate::error::ApiError;
use crate::models::{Comment, Notification, TaskAssignee, Watch};
use crate::time;

pub const TASK: &str = "task";
pub const PAGE: &str = "page";

/// What to tell people about.
pub struct Notice {
    pub workspace_id: uuid::Uuid,
    pub kind: &'static str,
    pub target_type: &'static str,
    pub target_id: uuid::Uuid,
    pub actor: Option<uuid::Uuid>,
    pub data: Value,
}

/// Kinds where a burst of changes is one notification: while the earlier one is unread it is refreshed.
fn coalesces(kind: &str) -> bool {
    matches!(kind, "task_changed" | "page_changed")
}

/// Notify `recipients` (the actor is never told about their own doing; people who cannot see the workspace
/// are skipped; duplicates are collapsed).
pub async fn notify(
    db: &mut dyn toasty::Executor,
    recipients: impl IntoIterator<Item = uuid::Uuid>,
    notice: Notice,
) -> Result<(), ApiError> {
    let mut seen: Vec<uuid::Uuid> = vec![];
    for user_id in recipients {
        if Some(user_id) == notice.actor || seen.contains(&user_id) {
            continue;
        }
        seen.push(user_id);
        if !access::can_see_workspace(db, user_id, notice.workspace_id).await? {
            continue;
        }
        if coalesces(notice.kind)
            && let Some(existing) = Notification::filter_by_user_id(user_id)
                .exec(db)
                .await?
                .into_iter()
                .find(|n| {
                    n.read_at.is_none()
                        && n.kind == notice.kind
                        && n.target_id == Some(notice.target_id)
                })
        {
            Notification::filter_by_id(existing.id)
                .update()
                .created_at(time::now())
                .actor_id(notice.actor)
                .data(notice.data.to_string())
                .exec(db)
                .await?;
            continue;
        }
        toasty::create!(Notification {
            user_id,
            kind: notice.kind.to_string(),
            workspace_id: notice.workspace_id,
            target_type: Some(notice.target_type.to_string()),
            target_id: Some(notice.target_id),
            actor_id: notice.actor,
            data: notice.data.to_string(),
            created_at: time::now(),
        })
        .exec(db)
        .await?;
    }
    Ok(())
}

/// The people watching a task or page.
pub async fn watchers(
    db: &mut dyn toasty::Executor,
    target_type: &str,
    target_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, ApiError> {
    Ok(Watch::filter_by_target_id(target_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|w| w.target_type == target_type)
        .map(|w| w.user_id)
        .collect())
}

/// Who cares about a task: its assignees and its watchers.
pub async fn task_audience(
    db: &mut dyn toasty::Executor,
    task_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, ApiError> {
    let mut out: Vec<uuid::Uuid> = TaskAssignee::filter_by_task_id(task_id)
        .exec(db)
        .await?
        .into_iter()
        .map(|a| a.user_id)
        .collect();
    out.extend(watchers(db, TASK, task_id).await?);
    Ok(out)
}

/// People who commented on a task before (they are told about later comments).
pub async fn commenters(
    db: &mut dyn toasty::Executor,
    task_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, ApiError> {
    Ok(Comment::filter_by_task_id(task_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|c| c.deleted_at.is_none())
        .map(|c| c.author_id)
        .collect())
}

/// Tell the audience of a task that something changed.
pub async fn task_changed(
    db: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    task_id: uuid::Uuid,
    actor: uuid::Uuid,
    title: &str,
    what: &str,
) -> Result<(), ApiError> {
    let audience = task_audience(db, task_id).await?;
    notify(
        db,
        audience,
        Notice {
            workspace_id,
            kind: "task_changed",
            target_type: TASK,
            target_id: task_id,
            actor: Some(actor),
            data: json!({ "title": title, "project_id": project_id, "what": what }),
        },
    )
    .await
}

/// A one-line description of a notification, for email.
pub fn describe(kind: &str, actor: Option<&str>, data: &Value) -> String {
    let title = data["title"].as_str().unwrap_or("something");
    let who = actor.unwrap_or("Someone");
    match kind {
        "task_assigned" => format!("{who} assigned you to \"{title}\""),
        "comment_added" => format!(
            "{who} commented on \"{title}\": {}",
            data["preview"].as_str().unwrap_or("")
        ),
        "mentioned" => format!("{who} mentioned a task you follow in the page \"{title}\""),
        "task_changed" => format!(
            "{who} changed \"{title}\" ({})",
            data["what"].as_str().unwrap_or("updated")
        ),
        "page_changed" => format!("{who} edited the page \"{title}\""),
        "due_soon" => format!("\"{title}\" is due tomorrow"),
        "overdue" => format!("\"{title}\" is overdue"),
        _ => format!("{who} did something with \"{title}\""),
    }
}

/// Where in the app a notification leads, as a path (`/w/<workspace>/...`).
pub fn path(
    workspace_id: uuid::Uuid,
    target_type: Option<&str>,
    target_id: Option<uuid::Uuid>,
) -> String {
    match (target_type, target_id) {
        (Some(TASK), Some(id)) => format!("/w/{workspace_id}/t/{id}"),
        (Some(PAGE), Some(id)) => format!("/w/{workspace_id}/wiki/{id}"),
        _ => format!("/w/{workspace_id}"),
    }
}
