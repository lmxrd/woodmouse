//! Integrity check. Toasty enforces relations in its query engine, not as database constraints, so rows
//! can be left pointing at nothing (a bug, a manual edit, a restored partial backup). This reports them.
//! Polymorphic targets where a missing target is allowed (`ContentLink.target_id`, audit log ids) are
//! not checked.

use std::collections::HashSet;

use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ApiError;
use crate::models::{
    CalendarLink, Comment, ContentLink, File, FileRef, FileVersion, Label, List, PageDoc, Project,
    Revision, SavedView, Session, Sprint, Task, TaskAssignee, TaskLabel, TaskLink, User, WikiPage,
    Workspace, WorkspaceMember,
};

/// One row that refers to something that does not exist.
#[derive(Serialize, ToSchema, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The table of the row, e.g. `tasks`.
    pub table: String,
    pub id: String,
    /// What is missing, e.g. `project_id -> projects`.
    pub relation: String,
}

struct Report(Vec<Problem>);

impl Report {
    fn check(
        &mut self,
        table: &str,
        id: impl ToString,
        relation: &str,
        target: uuid::Uuid,
        known: &HashSet<uuid::Uuid>,
    ) {
        if !known.contains(&target) {
            self.0.push(Problem {
                table: table.to_string(),
                id: id.to_string(),
                relation: relation.to_string(),
            });
        }
    }
}

fn ids<T>(rows: &[T], id: impl Fn(&T) -> uuid::Uuid) -> HashSet<uuid::Uuid> {
    rows.iter().map(id).collect()
}

/// Every orphaned row in the database.
pub async fn check(db: &mut dyn toasty::Executor) -> Result<Vec<Problem>, ApiError> {
    let users = User::all().exec(db).await?;
    let workspaces = Workspace::all().exec(db).await?;
    let projects = Project::all().exec(db).await?;
    let lists = List::all().exec(db).await?;
    let tasks = Task::all().exec(db).await?;
    let pages = WikiPage::all().exec(db).await?;
    let files = File::all().exec(db).await?;
    let labels = Label::all().exec(db).await?;

    let user_ids = ids(&users, |u| u.id);
    let workspace_ids = ids(&workspaces, |w| w.id);
    let project_ids = ids(&projects, |p| p.id);
    let list_ids = ids(&lists, |l| l.id);
    let task_ids = ids(&tasks, |t| t.id);
    let page_ids = ids(&pages, |p| p.id);
    let file_ids = ids(&files, |f| f.id);
    let label_ids = ids(&labels, |l| l.id);

    let mut r = Report(vec![]);
    for s in Session::all().exec(db).await? {
        r.check("sessions", s.id, "user_id -> users", s.user_id, &user_ids);
    }
    for m in WorkspaceMember::all().exec(db).await? {
        let id = format!("{}/{}", m.workspace_id, m.user_id);
        r.check(
            "workspace_members",
            &id,
            "user_id -> users",
            m.user_id,
            &user_ids,
        );
        r.check(
            "workspace_members",
            &id,
            "workspace_id -> workspaces",
            m.workspace_id,
            &workspace_ids,
        );
    }
    for p in &projects {
        r.check(
            "projects",
            p.id,
            "workspace_id -> workspaces",
            p.workspace_id,
            &workspace_ids,
        );
    }
    let sprints = Sprint::all().exec(db).await?;
    let sprint_ids = ids(&sprints, |s| s.id);
    for s in &sprints {
        r.check(
            "sprints",
            s.id,
            "project_id -> projects",
            s.project_id,
            &project_ids,
        );
    }
    for l in &lists {
        r.check(
            "lists",
            l.id,
            "project_id -> projects",
            l.project_id,
            &project_ids,
        );
    }
    for t in &tasks {
        r.check(
            "tasks",
            t.id,
            "project_id -> projects",
            t.project_id,
            &project_ids,
        );
        if let Some(list) = t.list_id {
            r.check("tasks", t.id, "list_id -> lists", list, &list_ids);
        }
        if let Some(parent) = t.parent_id {
            r.check("tasks", t.id, "parent_id -> tasks", parent, &task_ids);
        }
        if let Some(sprint) = t.sprint_id {
            r.check("tasks", t.id, "sprint_id -> sprints", sprint, &sprint_ids);
        }
    }
    for l in &labels {
        r.check(
            "labels",
            l.id,
            "workspace_id -> workspaces",
            l.workspace_id,
            &workspace_ids,
        );
    }
    for a in TaskAssignee::all().exec(db).await? {
        let id = format!("{}/{}", a.task_id, a.user_id);
        r.check(
            "task_assignees",
            &id,
            "task_id -> tasks",
            a.task_id,
            &task_ids,
        );
        r.check(
            "task_assignees",
            &id,
            "user_id -> users",
            a.user_id,
            &user_ids,
        );
    }
    for a in TaskLabel::all().exec(db).await? {
        let id = format!("{}/{}", a.task_id, a.label_id);
        r.check("task_labels", &id, "task_id -> tasks", a.task_id, &task_ids);
        r.check(
            "task_labels",
            &id,
            "label_id -> labels",
            a.label_id,
            &label_ids,
        );
    }
    for l in TaskLink::all().exec(db).await? {
        r.check("task_links", l.id, "task_id -> tasks", l.task_id, &task_ids);
        r.check(
            "task_links",
            l.id,
            "related_task_id -> tasks",
            l.related_task_id,
            &task_ids,
        );
    }
    for c in Comment::all().exec(db).await? {
        r.check("comments", c.id, "task_id -> tasks", c.task_id, &task_ids);
        r.check(
            "comments",
            c.id,
            "author_id -> users",
            c.author_id,
            &user_ids,
        );
    }
    for f in &files {
        r.check(
            "files",
            f.id,
            "workspace_id -> workspaces",
            f.workspace_id,
            &workspace_ids,
        );
    }
    for v in FileVersion::all().exec(db).await? {
        r.check(
            "file_versions",
            v.id,
            "file_id -> files",
            v.file_id,
            &file_ids,
        );
    }
    for f in FileRef::all().exec(db).await? {
        let id = format!("{}/{}/{}", f.file_id, f.target_type, f.target_id);
        r.check("file_refs", &id, "file_id -> files", f.file_id, &file_ids);
        let targets = if f.target_type == "page" {
            &page_ids
        } else {
            &task_ids
        };
        r.check(
            "file_refs",
            &id,
            "target_id -> target",
            f.target_id,
            targets,
        );
    }
    for p in &pages {
        r.check(
            "wiki_pages",
            p.id,
            "workspace_id -> workspaces",
            p.workspace_id,
            &workspace_ids,
        );
        if let Some(parent) = p.parent_id {
            r.check(
                "wiki_pages",
                p.id,
                "parent_id -> wiki_pages",
                parent,
                &page_ids,
            );
        }
    }
    for l in ContentLink::all().exec(db).await? {
        r.check(
            "content_links",
            l.id,
            "source_page_id -> wiki_pages",
            l.source_page_id,
            &page_ids,
        );
    }
    for d in PageDoc::all().exec(db).await? {
        r.check(
            "page_docs",
            d.page_id,
            "page_id -> wiki_pages",
            d.page_id,
            &page_ids,
        );
    }
    for v in SavedView::all().exec(db).await? {
        r.check(
            "saved_views",
            v.id,
            "project_id -> projects",
            v.project_id,
            &project_ids,
        );
        r.check(
            "saved_views",
            v.id,
            "owner_id -> users",
            v.owner_id,
            &user_ids,
        );
    }
    for c in CalendarLink::all().exec(db).await? {
        r.check(
            "calendar_links",
            c.id,
            "user_id -> users",
            c.user_id,
            &user_ids,
        );
    }
    for v in Revision::all().exec(db).await? {
        let targets = match v.target_type.as_str() {
            "page" => &page_ids,
            "comment" => continue, // comments are checked above; their ids are not loaded twice
            _ => &task_ids,
        };
        r.check(
            "revisions",
            v.id,
            "target_id -> target",
            v.target_id,
            targets,
        );
        r.check(
            "revisions",
            v.id,
            "actor_id -> users",
            v.actor_id,
            &user_ids,
        );
    }
    Ok(r.0)
}
