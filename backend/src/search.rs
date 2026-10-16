//! Full-text search over pages and tasks (SQLite FTS5).
//!
//! `search_docs` holds one row of searchable text per page and task, and `search_fts` indexes it
//! (external-content table, kept in sync by triggers). Triggers on `tasks` and on page titles keep the
//! text current without any handler having to remember; a page's body is written by
//! [`index_page_body`] when its blocks are saved. Which results a person may see is decided at query
//! time, by joining the live pages, tasks, lists and projects of the workspaces they can access, so
//! soft deletes, restores and moves never need to touch the index.

use crate::blocks::BlockData;
use crate::error::ApiError;
use toasty::stmt::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS search_docs (
        rid INTEGER PRIMARY KEY,
        id TEXT NOT NULL UNIQUE,
        kind TEXT NOT NULL,
        ws TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL DEFAULT ''
    )",
    "CREATE INDEX IF NOT EXISTS search_docs_ws ON search_docs(ws)",
    "CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
        title, body, content='search_docs', content_rowid='rid',
        tokenize='unicode61 remove_diacritics 2', prefix='2 3'
    )",
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
    // Tasks: the workspace comes from the task's project.
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

fn err(what: &str, e: impl std::fmt::Display) -> ApiError {
    tracing::error!(%e, "{what}");
    ApiError::internal("search is unavailable")
}

/// The id in the form used by the index (lowercase hex of the stored bytes).
fn key(id: uuid::Uuid) -> String {
    id.simple().to_string()
}

/// Create the index and its triggers (idempotent).
pub async fn ensure_index(db: &mut dyn toasty::Executor) -> Result<(), ApiError> {
    for sql in SETUP {
        toasty::sql::statement(*sql)
            .exec(db)
            .await
            .map_err(|e| err("creating the search index", e))?;
    }
    Ok(())
}

async fn set_body(
    db: &mut dyn toasty::Executor,
    page_id: uuid::Uuid,
    body: &str,
) -> Result<(), ApiError> {
    toasty::sql::statement("UPDATE search_docs SET body = ?1 WHERE id = ?2")
        .bind(body.to_string())
        .bind(key(page_id))
        .exec(db)
        .await
        .map_err(|e| err("indexing a page", e))?;
    Ok(())
}

/// Index a page's text after its blocks were saved.
pub async fn index_page_body(
    db: &mut dyn toasty::Executor,
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

fn str_at(row: &Value, col: usize) -> String {
    match row {
        Value::Record(rec) => match &rec[col] {
            Value::String(s) => s.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn i64_at(row: &Value, col: usize) -> i64 {
    match row {
        Value::Record(rec) => match &rec[col] {
            Value::I64(n) => *n,
            Value::Bool(b) => i64::from(*b),
            _ => 0,
        },
        _ => 0,
    }
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
    db: &mut dyn toasty::Executor,
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
    let marks: Vec<String> = (0..workspaces.len())
        .map(|i| format!("?{}", i + 4))
        .collect();
    let marks = marks.join(", ");
    let (from, order, snippet) = if expression.is_some() {
        (
            "search_fts JOIN search_docs d ON d.rid = search_fts.rowid",
            "bm25(search_fts, 10.0, 1.0)",
            "snippet(search_fts, 1, '', '', '…', 12)",
        )
    } else {
        ("search_docs d", "-d.rid", "''")
    };
    let matching = if expression.is_some() {
        "search_fts MATCH ?1 AND"
    } else {
        "?1 = ?1 AND"
    };
    // Matching documents first (cheap: the index and the workspace column), then only those are checked
    // against the live pages, tasks and comments, so deleted things and moved or deleted lists never show.
    let sql = format!(
        "WITH hits AS (
            SELECT d.id AS id, d.kind AS kind, d.ws AS ws, {snippet} AS snip, {order} AS rank
            FROM {from}
            WHERE {matching} d.ws IN ({marks})
              AND (?2 = '' OR d.kind = ?2 OR (?2 = 'task' AND d.kind = 'comment'))
            ORDER BY rank LIMIT ?3
        )
        SELECT h.kind, h.id, p.title, h.snip, h.ws, '', 0, h.rank
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
        ORDER BY rank"
    );
    let mut q = toasty::sql::query(sql)
        .bind(expression.unwrap_or_default())
        .bind(kind_filter.to_string())
        .bind((limit * 4) as i64);
    for ws in workspaces {
        q = q.bind(key(*ws));
    }
    let rows = q.exec(db).await.map_err(|e| err("searching", e))?;
    let parse = |s: String| uuid::Uuid::parse_str(&s).ok();
    let mut seen = std::collections::HashSet::new();
    let mut hits: Vec<Hit> = rows
        .iter()
        .filter_map(|row| {
            // A comment is found as the task it belongs to.
            let is_task = matches!(str_at(row, 0).as_str(), "task" | "comment");
            let snippet = Some(str_at(row, 3)).filter(|s| !s.trim().is_empty());
            Some(Hit {
                kind: if is_task { Kind::Task } else { Kind::Page },
                id: parse(str_at(row, 1))?,
                title: str_at(row, 2),
                snippet,
                workspace_id: parse(str_at(row, 4))?,
                project_id: if is_task { parse(str_at(row, 5)) } else { None },
                is_completed: is_task.then(|| i64_at(row, 6) != 0),
            })
        })
        .filter(|hit| seen.insert(hit.id))
        .collect();
    // A task matching in its text and in comments is one result; the best match came first.
    hits.truncate(limit);
    Ok(hits)
}
