//! Soft deletes. Nothing the API deletes is destroyed: rows get `deleted_at`/`deleted_by` and
//! disappear from every normal query, and Trash can restore them.
//!
//! Deleting a project, list or task cascades to what it contains, stamping the children with the
//! same `deleted_at`. Restoring a root restores exactly the children that carry its timestamp, so
//! something deleted separately earlier stays in the trash. (Two separate deletions within the
//! same second could be restored together; that is accepted at this scale.)

use std::collections::HashSet;

use toasty::stmt::in_list;

use crate::error::ApiError;
use crate::models::{Label, List, Project, Task};

type Tx<'a> = toasty::Transaction<'a>;

/// All tasks of a list that are `root` or sit below it at any depth. Subtasks live in their
/// parent's list, so the whole tree is found among the list's tasks.
pub(crate) fn subtree(list_tasks: &[Task], root: uuid::Uuid) -> Vec<uuid::Uuid> {
    let mut ids = vec![root];
    let mut seen: HashSet<uuid::Uuid> = HashSet::from([root]);
    loop {
        let next: Vec<uuid::Uuid> = list_tasks
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
    tx: &mut Tx<'_>,
    ids: Vec<uuid::Uuid>,
    stamp: Option<(i64, uuid::Uuid)>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    Task::all()
        .filter(in_list(Task::fields().id(), ids))
        .update()
        .deleted_at(stamp.map(|s| s.0))
        .deleted_by(stamp.map(|s| s.1))
        .exec(tx)
        .await?;
    Ok(())
}

async fn stamp_lists(
    tx: &mut Tx<'_>,
    ids: Vec<uuid::Uuid>,
    stamp: Option<(i64, uuid::Uuid)>,
) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    List::all()
        .filter(in_list(List::fields().id(), ids))
        .update()
        .deleted_at(stamp.map(|s| s.0))
        .deleted_by(stamp.map(|s| s.1))
        .exec(tx)
        .await?;
    Ok(())
}

/// Delete a task and its subtasks (any depth) that are not already deleted.
pub async fn delete_task(
    tx: &mut Tx<'_>,
    task: &Task,
    by: uuid::Uuid,
    now: i64,
) -> Result<(), ApiError> {
    let in_list_tasks = Task::filter_by_list_id(task.list_id).exec(tx).await?;
    let ids: Vec<uuid::Uuid> = subtree(&in_list_tasks, task.id)
        .into_iter()
        .filter(|id| {
            in_list_tasks
                .iter()
                .any(|t| t.id == *id && t.deleted_at.is_none())
        })
        .collect();
    stamp_tasks(tx, ids, Some((now, by))).await
}

/// Delete a list and its tasks that are not already deleted.
pub async fn delete_list(
    tx: &mut Tx<'_>,
    list_id: uuid::Uuid,
    by: uuid::Uuid,
    now: i64,
) -> Result<(), ApiError> {
    let open_tasks: Vec<uuid::Uuid> = Task::filter_by_list_id(list_id)
        .exec(tx)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at.is_none())
        .map(|t| t.id)
        .collect();
    stamp_tasks(tx, open_tasks, Some((now, by))).await?;
    stamp_lists(tx, vec![list_id], Some((now, by))).await
}

/// Delete a project, and its lists and tasks that are not already deleted.
pub async fn delete_project(
    tx: &mut Tx<'_>,
    project_id: uuid::Uuid,
    by: uuid::Uuid,
    now: i64,
) -> Result<(), ApiError> {
    let lists = List::filter_by_project_id(project_id).exec(tx).await?;
    for list in lists.iter().filter(|l| l.deleted_at.is_none()) {
        delete_list(tx, list.id, by, now).await?;
    }
    Project::filter_by_id(project_id)
        .update()
        .deleted_at(Some(now))
        .deleted_by(Some(by))
        .exec(tx)
        .await?;
    Ok(())
}

/// Delete a label (tasks keep the association; it reappears on restore).
pub async fn delete_label(
    tx: &mut Tx<'_>,
    label_id: uuid::Uuid,
    by: uuid::Uuid,
    now: i64,
) -> Result<(), ApiError> {
    Label::filter_by_id(label_id)
        .update()
        .deleted_at(Some(now))
        .deleted_by(Some(by))
        .exec(tx)
        .await?;
    Ok(())
}

/// Restore a task and the subtasks deleted with it. The list, project and parent must be live.
pub async fn restore_task(tx: &mut Tx<'_>, task: &Task) -> Result<(), ApiError> {
    let stamp = task.deleted_at;
    let list = List::get_by_id(tx, &task.list_id).await?;
    let project = Project::get_by_id(tx, &list.project_id).await?;
    if list.deleted_at.is_some() || project.deleted_at.is_some() {
        return Err(ApiError::conflict("restore its list or project first"));
    }
    if let Some(parent_id) = task.parent_id
        && Task::get_by_id(tx, &parent_id).await?.deleted_at.is_some()
    {
        return Err(ApiError::conflict("restore the parent task first"));
    }
    let in_list_tasks = Task::filter_by_list_id(task.list_id).exec(tx).await?;
    let ids: Vec<uuid::Uuid> = subtree(&in_list_tasks, task.id)
        .into_iter()
        .filter(|id| {
            in_list_tasks
                .iter()
                .any(|t| t.id == *id && t.deleted_at == stamp)
        })
        .collect();
    stamp_tasks(tx, ids, None).await
}

/// Restore a list and the tasks deleted with it. The project must be live.
pub async fn restore_list(tx: &mut Tx<'_>, list: &List) -> Result<(), ApiError> {
    let stamp = list.deleted_at;
    if Project::get_by_id(tx, &list.project_id)
        .await?
        .deleted_at
        .is_some()
    {
        return Err(ApiError::conflict("restore its project first"));
    }
    let ids: Vec<uuid::Uuid> = Task::filter_by_list_id(list.id)
        .exec(tx)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at == stamp)
        .map(|t| t.id)
        .collect();
    stamp_tasks(tx, ids, None).await?;
    stamp_lists(tx, vec![list.id], None).await
}

/// Restore a project and the lists and tasks deleted with it.
pub async fn restore_project(tx: &mut Tx<'_>, project: &Project) -> Result<(), ApiError> {
    let stamp = project.deleted_at;
    let lists = List::filter_by_project_id(project.id).exec(tx).await?;
    for list in lists.iter().filter(|l| l.deleted_at == stamp) {
        let ids: Vec<uuid::Uuid> = Task::filter_by_list_id(list.id)
            .exec(tx)
            .await?
            .into_iter()
            .filter(|t| t.deleted_at == stamp)
            .map(|t| t.id)
            .collect();
        stamp_tasks(tx, ids, None).await?;
        stamp_lists(tx, vec![list.id], None).await?;
    }
    Project::filter_by_id(project.id)
        .update()
        .deleted_at(None::<i64>)
        .deleted_by(None::<uuid::Uuid>)
        .exec(tx)
        .await?;
    Ok(())
}

pub async fn restore_label(tx: &mut Tx<'_>, label_id: uuid::Uuid) -> Result<(), ApiError> {
    Label::filter_by_id(label_id)
        .update()
        .deleted_at(None::<i64>)
        .deleted_by(None::<uuid::Uuid>)
        .exec(tx)
        .await?;
    Ok(())
}
