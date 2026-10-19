//! Soft deletes. Nothing the API deletes is destroyed: rows get `deleted_at` and disappear from every
//! normal query, and Trash can restore them. Who deleted something is in the audit log (the callers
//! record it in the same transaction).
//!
//! Deleting a project, list or task cascades to what it contains, stamping the children with the
//! same `deleted_at`. Restoring a root restores exactly the children that carry its timestamp, so
//! something deleted separately earlier stays in the trash.

use crate::db::prelude::*;
use entity::{files, labels, sprints, workspaces};
use entity::{lists, projects, tasks, wiki_pages};
use std::collections::HashSet;

use crate::time::DateTime;

use crate::db::Db;
use crate::error::ApiError;
use entity::{File, List, Project, Sprint, Task, WikiPage};

/// All tasks among `tasks` that are `root` or sit below it at any depth. Subtasks share their parent's
/// project, so the whole tree is found among the project's tasks.
pub(crate) fn subtree(tasks: &[Task], root: uuid::Uuid) -> Vec<uuid::Uuid> {
    let mut ids = vec![root];
    let mut seen: HashSet<uuid::Uuid> = HashSet::from([root]);
    loop {
        let next: Vec<uuid::Uuid> = tasks
            .iter()
            .filter(|t| t.parent_id.is_some_and(|p| seen.contains(&p)) && !seen.contains(&t.id))
            .map(|t| t.id)
            .collect();
        if next.is_empty() {
            return ids;
        }
        seen.extend(&next);
        ids.extend(next);
    }
}

async fn stamp_tasks(
    db: &impl Db,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    tasks::Entity::update_many()
        .set(tasks::ActiveModel {
            deleted_at: Set(stamp),
            ..Default::default()
        })
        .filter(tasks::Column::Id.is_in(ids))
        .exec(db)
        .await?;
    Ok(())
}

async fn stamp_lists(
    db: &impl Db,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    lists::Entity::update_many()
        .set(lists::ActiveModel {
            deleted_at: Set(stamp),
            ..Default::default()
        })
        .filter(lists::Column::Id.is_in(ids))
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a task and its subtasks (any depth) that are not already deleted.
pub async fn delete_task(db: &impl Db, task: &Task, now: DateTime) -> Result<(), ApiError> {
    let tasks = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(task.project_id))
        .all(db)
        .await?;
    let ids: Vec<uuid::Uuid> = subtree(&tasks, task.id)
        .into_iter()
        .filter(|id| tasks.iter().any(|t| t.id == *id && t.deleted_at.is_none()))
        .collect();
    stamp_tasks(db, ids, Some(now)).await
}

/// Delete a list and its tasks that are not already deleted.
pub async fn delete_list(db: &impl Db, list: &List, now: DateTime) -> Result<(), ApiError> {
    let open: Vec<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(list.project_id))
        .all(db)
        .await?
        .into_iter()
        .filter(|t| t.list_id == Some(list.id) && t.deleted_at.is_none())
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, open, Some(now)).await?;
    stamp_lists(db, vec![list.id], Some(now)).await
}

/// Delete a project, and its lists and tasks (including the inbox) that are not already deleted.
pub async fn delete_project(
    db: &impl Db,
    project_id: uuid::Uuid,
    now: DateTime,
) -> Result<(), ApiError> {
    let lists: Vec<uuid::Uuid> = lists::Entity::find()
        .filter(lists::Column::ProjectId.eq(project_id))
        .filter(lists::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|l| l.id)
        .collect();
    let tasks: Vec<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, tasks, Some(now)).await?;
    stamp_lists(db, lists, Some(now)).await?;
    projects::ActiveModel {
        id: Unchanged(project_id),
        deleted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// Delete a file (soft): hidden everywhere until restored.
pub async fn delete_file(db: &impl Db, file: &File, now: DateTime) -> Result<(), ApiError> {
    files::ActiveModel {
        id: Unchanged(file.id),
        deleted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

pub async fn restore_file(db: &impl Db, file_id: uuid::Uuid) -> Result<(), ApiError> {
    files::ActiveModel {
        id: Unchanged(file_id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// Delete a sprint: its tasks go back to the backlog (restoring the sprint does not put them back).
pub async fn delete_sprint(db: &impl Db, sprint: &Sprint, now: DateTime) -> Result<(), ApiError> {
    let ids: Vec<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(sprint.project_id))
        .all(db)
        .await?
        .into_iter()
        .filter(|t| t.sprint_id == Some(sprint.id))
        .map(|t| t.id)
        .collect();
    if !ids.is_empty() {
        tasks::Entity::update_many()
            .set(tasks::ActiveModel {
                sprint_id: Set(None),
                ..Default::default()
            })
            .filter(tasks::Column::Id.is_in(ids))
            .exec(db)
            .await?;
    }
    sprints::ActiveModel {
        id: Unchanged(sprint.id),
        deleted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// Restore a deleted sprint (its project must be live).
pub async fn restore_sprint(db: &impl Db, sprint: &Sprint) -> Result<(), ApiError> {
    if projects::Entity::find_by_id(sprint.project_id)
        .one_or_not_found(db)
        .await?
        .deleted_at
        .is_some()
    {
        return Err(ApiError::conflict("restore its project first"));
    }
    sprints::ActiveModel {
        id: Unchanged(sprint.id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// Delete a label (tasks keep the association; it reappears on restore).
pub async fn delete_label(
    db: &impl Db,
    label_id: uuid::Uuid,
    now: DateTime,
) -> Result<(), ApiError> {
    labels::ActiveModel {
        id: Unchanged(label_id),
        deleted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// Restore a task and the subtasks deleted with it. Its list, project and parent must be live.
pub async fn restore_task(db: &impl Db, task: &Task) -> Result<(), ApiError> {
    let stamp = task.deleted_at;
    let project = projects::Entity::find_by_id(task.project_id)
        .one_or_not_found(db)
        .await?;
    if project.deleted_at.is_some() {
        return Err(ApiError::conflict("restore its project first"));
    }
    if let Some(list_id) = task.list_id
        && lists::Entity::find_by_id(list_id)
            .one_or_not_found(db)
            .await?
            .deleted_at
            .is_some()
    {
        return Err(ApiError::conflict("restore its list first"));
    }
    if let Some(parent_id) = task.parent_id
        && tasks::Entity::find_by_id(parent_id)
            .one_or_not_found(db)
            .await?
            .deleted_at
            .is_some()
    {
        return Err(ApiError::conflict("restore the parent task first"));
    }
    let tasks = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(task.project_id))
        .all(db)
        .await?;
    let ids: Vec<uuid::Uuid> = subtree(&tasks, task.id)
        .into_iter()
        .filter(|id| tasks.iter().any(|t| t.id == *id && t.deleted_at == stamp))
        .collect();
    stamp_tasks(db, ids, None).await
}

/// Restore a list and the tasks deleted with it. The project must be live.
pub async fn restore_list(db: &impl Db, list: &List) -> Result<(), ApiError> {
    let stamp = list.deleted_at;
    if projects::Entity::find_by_id(list.project_id)
        .one_or_not_found(db)
        .await?
        .deleted_at
        .is_some()
    {
        return Err(ApiError::conflict("restore its project first"));
    }
    let ids: Vec<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(list.project_id))
        .all(db)
        .await?
        .into_iter()
        .filter(|t| t.list_id == Some(list.id) && t.deleted_at == stamp)
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, ids, None).await?;
    stamp_lists(db, vec![list.id], None).await
}

/// Restore a project and the lists and tasks deleted with it.
pub async fn restore_project(db: &impl Db, project: &Project) -> Result<(), ApiError> {
    let stamp = project.deleted_at;
    let lists: Vec<uuid::Uuid> = lists::Entity::find()
        .filter(lists::Column::ProjectId.eq(project.id))
        .all(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at == stamp)
        .map(|l| l.id)
        .collect();
    let tasks: Vec<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project.id))
        .all(db)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at == stamp)
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, tasks, None).await?;
    stamp_lists(db, lists, None).await?;
    projects::ActiveModel {
        id: Unchanged(project.id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

pub async fn restore_label(db: &impl Db, label_id: uuid::Uuid) -> Result<(), ApiError> {
    labels::ActiveModel {
        id: Unchanged(label_id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

/// A page and every page below it, at any depth.
pub(crate) fn page_subtree(pages: &[WikiPage], root: uuid::Uuid) -> Vec<uuid::Uuid> {
    let mut ids = vec![root];
    let mut seen: HashSet<uuid::Uuid> = HashSet::from([root]);
    loop {
        let next: Vec<uuid::Uuid> = pages
            .iter()
            .filter(|p| {
                p.parent_id.is_some_and(|parent| seen.contains(&parent)) && !seen.contains(&p.id)
            })
            .map(|p| p.id)
            .collect();
        if next.is_empty() {
            return ids;
        }
        seen.extend(&next);
        ids.extend(next);
    }
}

async fn stamp_pages(
    db: &impl Db,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    wiki_pages::Entity::update_many()
        .set(wiki_pages::ActiveModel {
            deleted_at: Set(stamp),
            ..Default::default()
        })
        .filter(wiki_pages::Column::Id.is_in(ids))
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a page and the pages below it that are not already deleted. Returns the deleted ids.
pub async fn delete_page(
    db: &impl Db,
    page: &WikiPage,
    now: DateTime,
) -> Result<Vec<uuid::Uuid>, ApiError> {
    let all = wiki_pages::Entity::find()
        .filter(wiki_pages::Column::WorkspaceId.eq(page.workspace_id))
        .all(db)
        .await?;
    let ids: Vec<uuid::Uuid> = page_subtree(&all, page.id)
        .into_iter()
        .filter(|id| all.iter().any(|p| p.id == *id && p.deleted_at.is_none()))
        .collect();
    stamp_pages(db, ids.clone(), Some(now)).await?;
    Ok(ids)
}

/// Restore a page and the pages deleted with it. Its parent page must be live.
pub async fn restore_page(db: &impl Db, page: &WikiPage) -> Result<(), ApiError> {
    let stamp = page.deleted_at;
    if let Some(parent_id) = page.parent_id
        && wiki_pages::Entity::find_by_id(parent_id)
            .one_or_not_found(db)
            .await?
            .deleted_at
            .is_some()
    {
        return Err(ApiError::conflict("restore the parent page first"));
    }
    let all = wiki_pages::Entity::find()
        .filter(wiki_pages::Column::WorkspaceId.eq(page.workspace_id))
        .all(db)
        .await?;
    let ids: Vec<uuid::Uuid> = page_subtree(&all, page.id)
        .into_iter()
        .filter(|id| all.iter().any(|p| p.id == *id && p.deleted_at == stamp))
        .collect();
    stamp_pages(db, ids, None).await
}

/// Delete a workspace: it disappears for everyone; its contents stay untouched underneath, so restoring
/// brings everything back as it was.
pub async fn delete_workspace(
    db: &impl Db,
    workspace_id: uuid::Uuid,
    now: DateTime,
) -> Result<(), ApiError> {
    workspaces::ActiveModel {
        id: Unchanged(workspace_id),
        deleted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

pub async fn restore_workspace(db: &impl Db, workspace_id: uuid::Uuid) -> Result<(), ApiError> {
    workspaces::ActiveModel {
        id: Unchanged(workspace_id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}
