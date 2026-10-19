//! Building the export document.

use crate::db::Db;
use crate::db::prelude::*;
use entity::{
    comments, labels, lists, projects, sprints, task_assignees, task_labels, tasks, users,
    wiki_pages, workspace_members,
};
use std::collections::HashMap;

use crate::error::ApiError;
use crate::{pages, time};
use entity::{Comment, Label, List, Project, Sprint, Task, User, WikiPage, Workspace};

use super::*;

pub async fn build_export(
    db: &impl Db,
    workspace: &Workspace,
    parts: &Parts,
    project_ids: &[uuid::Uuid],
) -> Result<Export, ApiError> {
    let users: HashMap<uuid::Uuid, User> = users::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();
    let email_of = |id: &uuid::Uuid| users.get(id).map(|u| u.email.clone()).unwrap_or_default();
    let labels: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::WorkspaceId.eq(workspace.id))
        .filter(labels::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
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
        for m in workspace_members::Entity::find()
            .filter(workspace_members::Column::WorkspaceId.eq(workspace.id))
            .all(db)
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
        let mut projects: Vec<Project> = projects::Entity::find()
            .filter(projects::Column::WorkspaceId.eq(workspace.id))
            .all(db)
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
        let mut pages: Vec<WikiPage> = wiki_pages::Entity::find()
            .filter(wiki_pages::Column::WorkspaceId.eq(workspace.id))
            .filter(wiki_pages::Column::DeletedAt.is_null())
            .all(db)
            .await?
            .into_iter()
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
    db: &impl Db,
    project: &Project,
    labels: &[Label],
    email_of: &(dyn Fn(&uuid::Uuid) -> String + Sync),
) -> Result<ExportProject, ApiError> {
    let mut lists: Vec<List> = lists::Entity::find()
        .filter(lists::Column::ProjectId.eq(project.id))
        .filter(lists::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .collect();
    lists.sort_by_key(|l| (l.position, l.id));
    let sprints: Vec<Sprint> = sprints::Entity::find()
        .filter(sprints::Column::ProjectId.eq(project.id))
        .filter(sprints::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .collect();
    let mut tasks: Vec<Task> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(project.id))
        .filter(tasks::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
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
    let assignees = task_assignees::Entity::find().all(db).await?;
    let task_labels = task_labels::Entity::find().all(db).await?;
    let comments = comments::Entity::find().all(db).await?;
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
