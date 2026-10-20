//! Full-text search over pages and tasks (SQLite FTS5).
//!
//! `search_docs` holds one row of searchable text per page, task and comment, and `search_fts` indexes it
//! (an FTS5 external-content table); both, and the triggers that keep them current, are created by
//! `migration/src/m20260101_000001_init.sql`. Triggers on `tasks`, comments and page titles keep the text current without
//! any handler having to remember; a page's body is written by [`index_page_body`] when its blocks are
//! saved. Which results a person may see is decided at query time, by joining the live pages, tasks,
//! lists and projects of the workspaces they can access, so soft deletes, restores and moves never need
//! to touch the index.

use crate::blocks::BlockData;
use crate::db::{Db, json_ids};
use crate::error::ApiError;
use sea_orm::{DatabaseBackend, FromQueryResult, Statement};

/// The triggers that keep `search_docs` and the index current, as in `migration/src/m20260101_000001_init.sql`. They are
/// created again at startup (idempotent) because a migration that rebuilds `tasks`, `comments` or
/// `wiki_pages` (SQLite drops a table's triggers with it) would otherwise leave search silently stale. The
/// test `startup_triggers_match_the_migration` fails when this list and the migration drift apart.
const TRIGGERS: &[&str] = &[
    "CREATE TRIGGER IF NOT EXISTS search_docs_ai AFTER INSERT ON search_docs BEGIN
        INSERT INTO search_fts(rowid, title, body) VALUES (new.rid, new.title, new.body);
    END",
    "CREATE TRIGGER IF NOT EXISTS search_docs_ad AFTER DELETE ON search_docs BEGIN
        INSERT INTO search_fts(search_fts, rowid, title, body)
            VALUES ('delete', old.rid, old.title, old.body);
    END",
    "CREATE TRIGGER IF NOT EXISTS search_docs_au AFTER UPDATE ON search_docs BEGIN
        INSERT INTO search_fts(search_fts, rowid, title, body)
            VALUES ('delete', old.rid, old.title, old.body);
        INSERT INTO search_fts(rowid, title, body) VALUES (new.rid, new.title, new.body);
    END",
    "CREATE TRIGGER IF NOT EXISTS search_tasks_ai AFTER INSERT ON tasks BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'task',
                    (SELECT lower(hex(workspace_id)) FROM projects WHERE id = new.project_id),
                    new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_tasks_au AFTER UPDATE OF title, description ON tasks BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'task',
                    (SELECT lower(hex(workspace_id)) FROM projects WHERE id = new.project_id),
                    new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_comments_ai AFTER INSERT ON comments BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'comment',
                    (SELECT lower(hex(p.workspace_id)) FROM tasks t JOIN projects p ON p.id = t.project_id
                      WHERE t.id = new.task_id),
                    '', new.body)
            ON CONFLICT(id) DO UPDATE SET body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_comments_au AFTER UPDATE OF body ON comments BEGIN
        UPDATE search_docs SET body = new.body WHERE id = lower(hex(new.id));
    END",
    "CREATE TRIGGER IF NOT EXISTS search_pages_ai AFTER INSERT ON wiki_pages BEGIN
        INSERT INTO search_docs(id, kind, ws, title)
            VALUES (lower(hex(new.id)), 'page', lower(hex(new.workspace_id)), new.title)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_pages_au AFTER UPDATE OF title ON wiki_pages BEGIN
        UPDATE search_docs SET title = new.title WHERE id = lower(hex(new.id));
    END",
];

/// Create the search triggers again if they are missing. The migrations must have run.
pub async fn ensure_triggers(db: &impl Db) -> Result<(), ApiError> {
    for sql in TRIGGERS {
        db.execute_unprepared(sql)
            .await
            .map_err(|e| err("creating the search triggers", e))?;
    }
    Ok(())
}

fn err(what: &str, e: impl std::fmt::Display) -> ApiError {
    tracing::error!(%e, "{what}");
    ApiError::internal("search is unavailable")
}

/// The id in the form used by the index (lowercase hex of the stored bytes).
fn key(id: uuid::Uuid) -> String {
    id.simple().to_string()
}

async fn set_body(db: &impl Db, page_id: uuid::Uuid, body: &str) -> Result<(), ApiError> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE search_docs SET body = ?1 WHERE id = ?2",
        [body.into(), key(page_id).into()],
    ))
    .await
    .map_err(|e| err("indexing a page", e))?;
    Ok(())
}

/// Index a page's text after its blocks were saved.
pub async fn index_page_body(
    db: &impl Db,
    page_id: uuid::Uuid,
    blocks: &[BlockData],
) -> Result<(), ApiError> {
    let text = blocks
        .iter()
        .map(BlockData::plain_text)
        .collect::<Vec<_>>()
        .join("\n");
    set_body(db, page_id, &text).await
}

/// One row of the search query, before it is checked and typed.
#[derive(FromQueryResult)]
struct Found {
    kind: String,
    id: String,
    title: String,
    snip: String,
    ws: String,
    project: String,
    completed: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Page,
    Task,
}

pub struct Hit {
    pub kind: Kind,
    pub id: uuid::Uuid,
    pub title: String,
    pub snippet: Option<String>,
    pub workspace_id: uuid::Uuid,
    pub project_id: Option<uuid::Uuid>,
    pub is_completed: Option<bool>,
}

/// An FTS5 expression from what the person typed: every word must match, as a prefix, in the title or
/// the text. `None` when there are no searchable characters.
fn match_expression(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Search the live pages and tasks of `workspaces`, best match first (title matches count ten times as
/// much as text matches). An empty query returns the newest items instead.
pub async fn search(
    db: &impl Db,
    workspaces: &[uuid::Uuid],
    query: &str,
    kind: Option<Kind>,
    limit: usize,
) -> Result<Vec<Hit>, ApiError> {
    if workspaces.is_empty() {
        return Ok(vec![]);
    }
    let expression = if query.trim().is_empty() {
        None
    } else {
        match match_expression(query) {
            Some(e) => Some(e),
            None => return Ok(vec![]),
        }
    };
    let kind_filter = match kind {
        Some(Kind::Page) => "page",
        Some(Kind::Task) => "task",
        None => "",
    };
    // Matching documents first (cheap: the index and the workspace column), then only those are checked
    // against the live pages, tasks and comments, so deleted things and moved or deleted lists never show.
    // Two queries: with words to match (ranked, with a snippet), and without (the newest items). Both take
    // the kind (`''` for any), the limit and the workspaces as a JSON list of ids.
    const WITH_WORDS: &str = "WITH hits AS (
            SELECT d.id AS id, d.kind AS kind, d.ws AS ws, snippet(search_fts, 1, '', '', '…', 12) AS snip,
                   bm25(search_fts, 10.0, 1.0) AS rank
            FROM search_fts JOIN search_docs d ON d.rid = search_fts.rowid
            WHERE search_fts MATCH ?1 AND d.ws IN (SELECT value FROM json_each(?4))
              AND (?2 = '' OR d.kind = ?2 OR (?2 = 'task' AND d.kind = 'comment'))
            ORDER BY rank LIMIT ?3
        )
        SELECT h.kind AS kind, h.id AS id, p.title AS title, h.snip AS snip, h.ws AS ws, '' AS project,
               0 AS completed, h.rank AS rank
        FROM hits h JOIN wiki_pages p ON p.id = unhex(h.id)
        WHERE h.kind = 'page' AND p.deleted_at IS NULL
        UNION ALL
        SELECT 'task', lower(hex(t.id)), t.title, h.snip, h.ws, lower(hex(t.project_id)),
               t.completed_at IS NOT NULL, h.rank
        FROM hits h
        JOIN tasks t ON t.id = unhex(h.id)
        JOIN projects pr ON pr.id = t.project_id
        LEFT JOIN lists l ON l.id = t.list_id
        WHERE h.kind = 'task' AND t.deleted_at IS NULL AND pr.deleted_at IS NULL
          AND (l.id IS NULL OR l.deleted_at IS NULL)
        UNION ALL
        SELECT 'comment', lower(hex(t.id)), t.title, h.snip, h.ws, lower(hex(t.project_id)),
               t.completed_at IS NOT NULL, h.rank
        FROM hits h
        JOIN comments c ON c.id = unhex(h.id)
        JOIN tasks t ON t.id = c.task_id
        JOIN projects pr ON pr.id = t.project_id
        LEFT JOIN lists l ON l.id = t.list_id
        WHERE h.kind = 'comment' AND c.deleted_at IS NULL AND t.deleted_at IS NULL
          AND pr.deleted_at IS NULL AND (l.id IS NULL OR l.deleted_at IS NULL)
        ORDER BY rank";
    const NEWEST: &str = "WITH hits AS (
            SELECT d.id AS id, d.kind AS kind, d.ws AS ws, '' AS snip, -d.rid AS rank
            FROM search_docs d
            WHERE ?1 = ?1 AND d.ws IN (SELECT value FROM json_each(?4))
              AND (?2 = '' OR d.kind = ?2 OR (?2 = 'task' AND d.kind = 'comment'))
            ORDER BY rank LIMIT ?3
        )
        SELECT h.kind AS kind, h.id AS id, p.title AS title, h.snip AS snip, h.ws AS ws, '' AS project,
               0 AS completed, h.rank AS rank
        FROM hits h JOIN wiki_pages p ON p.id = unhex(h.id)
        WHERE h.kind = 'page' AND p.deleted_at IS NULL
        UNION ALL
        SELECT 'task', lower(hex(t.id)), t.title, h.snip, h.ws, lower(hex(t.project_id)),
               t.completed_at IS NOT NULL, h.rank
        FROM hits h
        JOIN tasks t ON t.id = unhex(h.id)
        JOIN projects pr ON pr.id = t.project_id
        LEFT JOIN lists l ON l.id = t.list_id
        WHERE h.kind = 'task' AND t.deleted_at IS NULL AND pr.deleted_at IS NULL
          AND (l.id IS NULL OR l.deleted_at IS NULL)
        UNION ALL
        SELECT 'comment', lower(hex(t.id)), t.title, h.snip, h.ws, lower(hex(t.project_id)),
               t.completed_at IS NOT NULL, h.rank
        FROM hits h
        JOIN comments c ON c.id = unhex(h.id)
        JOIN tasks t ON t.id = c.task_id
        JOIN projects pr ON pr.id = t.project_id
        LEFT JOIN lists l ON l.id = t.list_id
        WHERE h.kind = 'comment' AND c.deleted_at IS NULL AND t.deleted_at IS NULL
          AND pr.deleted_at IS NULL AND (l.id IS NULL OR l.deleted_at IS NULL)
        ORDER BY rank";
    let found: Vec<Found> = Found::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        if expression.is_some() {
            WITH_WORDS
        } else {
            NEWEST
        },
        [
            expression.unwrap_or_default().into(),
            kind_filter.into(),
            ((limit * 4) as i64).into(),
            json_ids(workspaces).into(),
        ],
    ))
    .all(db)
    .await
    .map_err(|e| err("searching", e))?;
    let parse = |s: &str| uuid::Uuid::parse_str(s).ok();
    let mut seen = std::collections::HashSet::new();
    let mut hits: Vec<Hit> = found
        .iter()
        .filter_map(|row| {
            // A comment is found as the task it belongs to.
            let is_task = matches!(row.kind.as_str(), "task" | "comment");
            let snippet = Some(row.snip.clone()).filter(|s| !s.trim().is_empty());
            Some(Hit {
                kind: if is_task { Kind::Task } else { Kind::Page },
                id: parse(&row.id)?,
                title: row.title.clone(),
                snippet,
                workspace_id: parse(&row.ws)?,
                project_id: if is_task { parse(&row.project) } else { None },
                is_completed: is_task.then_some(row.completed != 0),
            })
        })
        .filter(|hit| seen.insert(hit.id))
        .collect();
    // A task matching in its text and in comments is one result; the best match came first.
    hits.truncate(limit);
    Ok(hits)
}
