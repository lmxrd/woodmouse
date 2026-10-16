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
use crate::models::Page;
use toasty::stmt::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS search_docs (
        rid INTEGER PRIMARY KEY,
        id TEXT NOT NULL UNIQUE,
        kind TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL DEFAULT ''
    )",
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
    "CREATE TRIGGER IF NOT EXISTS search_tasks_ai AFTER INSERT ON tasks BEGIN
        INSERT INTO search_docs(id, kind, title, body)
            VALUES (lower(hex(new.id)), 'task', new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_tasks_au AFTER UPDATE OF title, description ON tasks BEGIN
        INSERT INTO search_docs(id, kind, title, body)
            VALUES (lower(hex(new.id)), 'task', new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_tasks_ad AFTER DELETE ON tasks BEGIN
        DELETE FROM search_docs WHERE id = lower(hex(old.id));
    END",
    "CREATE TRIGGER IF NOT EXISTS search_comments_ai AFTER INSERT ON comments BEGIN
        INSERT INTO search_docs(id, kind, title, body)
            VALUES (lower(hex(new.id)), 'comment', '', new.body)
            ON CONFLICT(id) DO UPDATE SET body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_comments_au AFTER UPDATE OF body ON comments BEGIN
        INSERT INTO search_docs(id, kind, title, body)
            VALUES (lower(hex(new.id)), 'comment', '', new.body)
            ON CONFLICT(id) DO UPDATE SET body = excluded.body;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_comments_ad AFTER DELETE ON comments BEGIN
        DELETE FROM search_docs WHERE id = lower(hex(old.id));
    END",
    "CREATE TRIGGER IF NOT EXISTS search_pages_ai AFTER INSERT ON pages BEGIN
        INSERT INTO search_docs(id, kind, title) VALUES (lower(hex(new.id)), 'page', new.title)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_pages_au AFTER UPDATE OF title ON pages BEGIN
        INSERT INTO search_docs(id, kind, title) VALUES (lower(hex(new.id)), 'page', new.title)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title;
    END",
    "CREATE TRIGGER IF NOT EXISTS search_pages_ad AFTER DELETE ON pages BEGIN
        DELETE FROM search_docs WHERE id = lower(hex(old.id));
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

/// Create the index and its triggers (idempotent), and index what existed before them.
pub async fn ensure_index(db: &mut toasty::Db) -> Result<(), ApiError> {
    for sql in SETUP {
        toasty::sql::statement(*sql)
            .exec(db)
            .await
            .map_err(|e| err("creating the search index", e))?;
    }
    // Comments joined the index after tasks and pages did: fill them in once for older databases.
    toasty::sql::statement(
        "INSERT INTO search_docs(id, kind, title, body)
            SELECT lower(hex(id)), 'comment', '', body FROM comments
            WHERE NOT EXISTS (SELECT 1 FROM search_docs WHERE kind = 'comment')
            ON CONFLICT(id) DO NOTHING",
    )
    .exec(db)
    .await
    .map_err(|e| err("filling the search index", e))?;
    let rows = toasty::sql::query("SELECT COUNT(*) FROM search_docs")
        .exec(db)
        .await
        .map_err(|e| err("reading the search index", e))?;
    if i64_at(&rows[0], 0) > 0 {
        return Ok(());
    }
    // First start with search: fill the index from existing data.
    for sql in [
        "INSERT INTO search_docs(id, kind, title, body)
            SELECT lower(hex(id)), 'task', title, description FROM tasks",
        "INSERT INTO search_docs(id, kind, title) SELECT lower(hex(id)), 'page', title FROM pages",
    ] {
        toasty::sql::statement(sql)
            .exec(db)
            .await
            .map_err(|e| err("filling the search index", e))?;
    }
    use crate::models::Block;
    for page in Page::all().exec(db).await? {
        let blocks = Block::filter_by_page_id(page.id).exec(db).await?;
        let body = blocks
            .iter()
            .filter_map(|b| {
                let kind = crate::blocks::BlockKind::parse(&b.kind)?;
                let spans = serde_json::from_str(&b.spans).ok()?;
                Some((
                    b.position,
                    BlockData {
                        kind,
                        attrs: Default::default(),
                        spans,
                    }
                    .plain_text(),
                ))
            })
            .collect::<Vec<_>>();
        let mut body = body;
        body.sort_by(|a, b| a.0.total_cmp(&b.0));
        let text = body
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n");
        set_body(db, page.id, &text).await?;
    }
    Ok(())
}

async fn set_body(db: &mut toasty::Db, page_id: uuid::Uuid, body: &str) -> Result<(), ApiError> {
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
    db: &mut toasty::Db,
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
    db: &mut toasty::Db,
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
    let sql = format!(
        "WITH scoped AS (
            SELECT lower(hex(p.id)) AS id, lower(hex(p.workspace_id)) AS ws,
                   '' AS project, 0 AS done, p.title AS title, lower(hex(p.id)) AS target
            FROM pages p
            WHERE p.deleted_at IS NULL AND lower(hex(p.workspace_id)) IN ({marks})
            UNION ALL
            SELECT lower(hex(t.id)), lower(hex(pr.workspace_id)), lower(hex(pr.id)), t.is_completed,
                   t.title, lower(hex(t.id))
            FROM tasks t
            JOIN lists l ON l.id = t.list_id
            JOIN projects pr ON pr.id = l.project_id
            WHERE t.deleted_at IS NULL AND l.deleted_at IS NULL AND pr.deleted_at IS NULL
              AND lower(hex(pr.workspace_id)) IN ({marks})
            UNION ALL
            SELECT lower(hex(c.id)), lower(hex(pr.workspace_id)), lower(hex(pr.id)), t.is_completed,
                   t.title, lower(hex(t.id))
            FROM comments c
            JOIN tasks t ON t.id = c.task_id
            JOIN lists l ON l.id = t.list_id
            JOIN projects pr ON pr.id = l.project_id
            WHERE c.deleted_at IS NULL AND t.deleted_at IS NULL AND l.deleted_at IS NULL
              AND pr.deleted_at IS NULL AND lower(hex(pr.workspace_id)) IN ({marks})
        )
        SELECT d.kind, s.target, s.title, {snippet}, s.ws, s.project, s.done
        FROM {from} JOIN scoped s ON s.id = d.id
        WHERE {matching} (?2 = '' OR d.kind = ?2 OR (?2 = 'task' AND d.kind = 'comment'))
        ORDER BY {order} LIMIT ?3"
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
