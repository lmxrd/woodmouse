//! Soft deletes. Nothing the API deletes is destroyed: rows get `deleted_at` and disappear from every
//! normal query, and Trash can restore them. Who deleted something is in the audit log (the callers
//! record it in the same transaction).
//!
//! Deleting a project, list or task cascades to what it contains, stamping the children with the
//! same `deleted_at`. Restoring a root restores exactly the children that carry its timestamp, so
//! something deleted separately earlier stays in the trash.

use std::collections::HashSet;

use jiff::civil::DateTime;
use toasty::stmt::in_list;

use crate::error::ApiError;
use crate::models::{File, Label, List, Project, Sprint, Task, WikiPage, Workspace};

type Db<'a> = &'a mut dyn toasty::Executor;

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
    db: Db<'_>,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    Task::all()
        .filter(in_list(Task::fields().id(), ids))
        .update()
        .deleted_at(stamp)
        .exec(db)
        .await?;
    Ok(())
}

async fn stamp_lists(
    db: Db<'_>,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    List::all()
        .filter(in_list(List::fields().id(), ids))
        .update()
        .deleted_at(stamp)
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a task and its subtasks (any depth) that are not already deleted.
pub async fn delete_task(db: Db<'_>, task: &Task, now: DateTime) -> Result<(), ApiError> {
    let tasks = Task::filter_by_project_id(task.project_id).exec(db).await?;
    let ids: Vec<uuid::Uuid> = subtree(&tasks, task.id)
        .into_iter()
        .filter(|id| tasks.iter().any(|t| t.id == *id && t.deleted_at.is_none()))
        .collect();
    stamp_tasks(db, ids, Some(now)).await
}

/// Delete a list and its tasks that are not already deleted.
pub async fn delete_list(db: Db<'_>, list: &List, now: DateTime) -> Result<(), ApiError> {
    let open: Vec<uuid::Uuid> = Task::filter_by_project_id(list.project_id)
        .exec(db)
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
    db: Db<'_>,
    project_id: uuid::Uuid,
    now: DateTime,
) -> Result<(), ApiError> {
    let lists: Vec<uuid::Uuid> = List::filter_by_project_id(project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .map(|l| l.id)
        .collect();
    let tasks: Vec<uuid::Uuid> = Task::filter_by_project_id(project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at.is_none())
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, tasks, Some(now)).await?;
    stamp_lists(db, lists, Some(now)).await?;
    Project::filter_by_id(project_id)
        .update()
        .deleted_at(Some(now))
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a file (soft): hidden everywhere until restored.
pub async fn delete_file(db: Db<'_>, file: &File, now: DateTime) -> Result<(), ApiError> {
    File::filter_by_id(file.id)
        .update()
        .deleted_at(Some(now))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn restore_file(db: Db<'_>, file_id: uuid::Uuid) -> Result<(), ApiError> {
    File::filter_by_id(file_id)
        .update()
        .deleted_at(None::<DateTime>)
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a sprint: its tasks go back to the backlog (restoring the sprint does not put them back).
pub async fn delete_sprint(db: Db<'_>, sprint: &Sprint, now: DateTime) -> Result<(), ApiError> {
    let ids: Vec<uuid::Uuid> = Task::filter_by_project_id(sprint.project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.sprint_id == Some(sprint.id))
        .map(|t| t.id)
        .collect();
    if !ids.is_empty() {
        Task::all()
            .filter(in_list(Task::fields().id(), ids))
            .update()
            .sprint_id(None::<uuid::Uuid>)
            .exec(db)
            .await?;
    }
    Sprint::filter_by_id(sprint.id)
        .update()
        .deleted_at(Some(now))
        .exec(db)
        .await?;
    Ok(())
}

/// Restore a deleted sprint (its project must be live).
pub async fn restore_sprint(db: Db<'_>, sprint: &Sprint) -> Result<(), ApiError> {
    if Project::get_by_id(db, &sprint.project_id)
        .await?
        .deleted_at
        .is_some()
    {
        return Err(ApiError::conflict("restore its project first"));
    }
    Sprint::filter_by_id(sprint.id)
        .update()
        .deleted_at(None::<DateTime>)
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a label (tasks keep the association; it reappears on restore).
pub async fn delete_label(db: Db<'_>, label_id: uuid::Uuid, now: DateTime) -> Result<(), ApiError> {
    Label::filter_by_id(label_id)
        .update()
        .deleted_at(Some(now))
        .exec(db)
        .await?;
    Ok(())
}

/// Restore a task and the subtasks deleted with it. Its list, project and parent must be live.
pub async fn restore_task(db: Db<'_>, task: &Task) -> Result<(), ApiError> {
    let stamp = task.deleted_at;
    let project = Project::get_by_id(db, &task.project_id).await?;
    if project.deleted_at.is_some() {
        return Err(ApiError::conflict("restore its project first"));
    }
    if let Some(list_id) = task.list_id
        && List::get_by_id(db, &list_id).await?.deleted_at.is_some()
    {
        return Err(ApiError::conflict("restore its list first"));
    }
    if let Some(parent_id) = task.parent_id
        && Task::get_by_id(db, &parent_id).await?.deleted_at.is_some()
    {
        return Err(ApiError::conflict("restore the parent task first"));
    }
    let tasks = Task::filter_by_project_id(task.project_id).exec(db).await?;
    let ids: Vec<uuid::Uuid> = subtree(&tasks, task.id)
        .into_iter()
        .filter(|id| tasks.iter().any(|t| t.id == *id && t.deleted_at == stamp))
        .collect();
    stamp_tasks(db, ids, None).await
}

/// Restore a list and the tasks deleted with it. The project must be live.
pub async fn restore_list(db: Db<'_>, list: &List) -> Result<(), ApiError> {
    let stamp = list.deleted_at;
    if Project::get_by_id(db, &list.project_id)
        .await?
        .deleted_at
        .is_some()
    {
        return Err(ApiError::conflict("restore its project first"));
    }
    let ids: Vec<uuid::Uuid> = Task::filter_by_project_id(list.project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.list_id == Some(list.id) && t.deleted_at == stamp)
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, ids, None).await?;
    stamp_lists(db, vec![list.id], None).await
}

/// Restore a project and the lists and tasks deleted with it.
pub async fn restore_project(db: Db<'_>, project: &Project) -> Result<(), ApiError> {
    let stamp = project.deleted_at;
    let lists: Vec<uuid::Uuid> = List::filter_by_project_id(project.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at == stamp)
        .map(|l| l.id)
        .collect();
    let tasks: Vec<uuid::Uuid> = Task::filter_by_project_id(project.id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at == stamp)
        .map(|t| t.id)
        .collect();
    stamp_tasks(db, tasks, None).await?;
    stamp_lists(db, lists, None).await?;
    Project::filter_by_id(project.id)
        .update()
        .deleted_at(None::<DateTime>)
        .exec(db)
        .await?;
    Ok(())
}

pub async fn restore_label(db: Db<'_>, label_id: uuid::Uuid) -> Result<(), ApiError> {
    Label::filter_by_id(label_id)
        .update()
        .deleted_at(None::<DateTime>)
        .exec(db)
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
    db: Db<'_>,
    ids: Vec<uuid::Uuid>,
    stamp: Option<DateTime>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    WikiPage::all()
        .filter(in_list(WikiPage::fields().id(), ids))
        .update()
        .deleted_at(stamp)
        .exec(db)
        .await?;
    Ok(())
}

/// Delete a page and the pages below it that are not already deleted. Returns the deleted ids.
pub async fn delete_page(
    db: Db<'_>,
    page: &WikiPage,
    now: DateTime,
) -> Result<Vec<uuid::Uuid>, ApiError> {
    let all = WikiPage::filter_by_workspace_id(page.workspace_id)
        .exec(db)
        .await?;
    let ids: Vec<uuid::Uuid> = page_subtree(&all, page.id)
        .into_iter()
        .filter(|id| all.iter().any(|p| p.id == *id && p.deleted_at.is_none()))
        .collect();
    stamp_pages(db, ids.clone(), Some(now)).await?;
    Ok(ids)
}

/// Restore a page and the pages deleted with it. Its parent page must be live.
pub async fn restore_page(db: Db<'_>, page: &WikiPage) -> Result<(), ApiError> {
    let stamp = page.deleted_at;
    if let Some(parent_id) = page.parent_id
        && WikiPage::get_by_id(db, &parent_id)
            .await?
            .deleted_at
            .is_some()
    {
        return Err(ApiError::conflict("restore the parent page first"));
    }
    let all = WikiPage::filter_by_workspace_id(page.workspace_id)
        .exec(db)
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
    db: Db<'_>,
    workspace_id: uuid::Uuid,
    now: DateTime,
) -> Result<(), ApiError> {
    Workspace::filter_by_id(workspace_id)
        .update()
        .deleted_at(Some(now))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn restore_workspace(db: Db<'_>, workspace_id: uuid::Uuid) -> Result<(), ApiError> {
    Workspace::filter_by_id(workspace_id)
        .update()
        .deleted_at(None::<DateTime>)
        .exec(db)
        .await?;
    Ok(())
}
