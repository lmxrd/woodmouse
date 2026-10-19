//! Integrity check. SQLite enforces the foreign keys, so a row can only point at nothing when something
//! wrote with `foreign_keys` off (a manual edit, a restored partial copy). `PRAGMA foreign_key_check` lists
//! those rows, whatever the setting. The polymorphic targets (a file reference's or a revision's `target_id`
//! is a task, page or comment, by its `target_type`) are not foreign keys, so they are checked here.

use std::collections::HashMap;

use serde::Serialize;
use utoipa::ToSchema;

use crate::db::Db;
use crate::error::ApiError;

/// One row that refers to something that does not exist.
#[derive(Serialize, ToSchema, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    /// The table of the row, e.g. `tasks`.
    pub table: String,
    pub id: String,
    /// What is missing, e.g. `project_id -> projects`.
    pub relation: String,
}

/// Every orphaned row in the database.
pub async fn check(db: &impl Db) -> Result<Vec<Problem>, ApiError> {
    let mut problems = foreign_key_problems(db).await?;
    problems.extend(polymorphic_problems(db).await?);
    problems.sort();
    Ok(problems)
}

/// The rows `PRAGMA foreign_key_check` reports, named by their primary key and the relation they break.
async fn foreign_key_problems(db: &impl Db) -> Result<Vec<Problem>, ApiError> {
    let violations: Vec<(String, Option<i64>, String, i64)> =
        crate::db::query(db, "PRAGMA foreign_key_check", []).await?;
    // Per table: its foreign keys (id, column, parent table) and its primary key columns.
    let mut keys: HashMap<String, Vec<(i64, String, String)>> = HashMap::new();
    let mut primaries: HashMap<String, Vec<String>> = HashMap::new();
    let mut problems = vec![];
    for (table, rowid, parent, fkid) in violations {
        if !keys.contains_key(&table) {
            // The names come from SQLite itself, never from a request.
            let list: Vec<(i64, String, String)> = crate::db::query(
                db,
                format!("SELECT id, \"from\", \"table\" FROM pragma_foreign_key_list('{table}')"),
                [],
            )
            .await?;
            let pk: Vec<(String,)> = crate::db::query(
                db,
                format!("SELECT name FROM pragma_table_info('{table}') WHERE pk > 0 ORDER BY pk"),
                [],
            )
            .await?;
            keys.insert(table.clone(), list);
            primaries.insert(table.clone(), pk.into_iter().map(|(n,)| n).collect());
        }
        let column = keys[&table]
            .iter()
            .find(|(id, _, _)| *id == fkid)
            .map(|(_, from, _)| from.clone())
            .unwrap_or_default();
        let id = match rowid {
            Some(rowid) => primary_key(db, &table, &primaries[&table], rowid).await?,
            None => String::new(),
        };
        problems.push(Problem {
            table,
            id,
            relation: format!("{column} -> {parent}"),
        });
    }
    Ok(problems)
}

/// The row's primary key as text: a UUID in its usual form, parts of a composite key joined by `/`.
async fn primary_key(
    db: &impl Db,
    table: &str,
    columns: &[String],
    rowid: i64,
) -> Result<String, ApiError> {
    if columns.is_empty() {
        return Ok(rowid.to_string());
    }
    let parts: Vec<String> = columns
        .iter()
        .map(|c| {
            format!(
                "CASE WHEN typeof(\"{c}\") = 'blob' THEN lower(hex(\"{c}\")) ELSE CAST(\"{c}\" AS TEXT) END"
            )
        })
        .collect();
    let sql = format!(
        "SELECT {} FROM \"{table}\" WHERE rowid = ?1",
        parts.join(" || '/' || ")
    );
    let key: Vec<(String,)> = crate::db::query(db, sql, [rowid.into()]).await?;
    Ok(key
        .into_iter()
        .next()
        .map(|(k,)| uuids(&k))
        .unwrap_or_default())
}

/// Hex ids (32 characters) written the usual way, anything else as it is.
fn uuids(text: &str) -> String {
    text.split('/')
        .map(|part| {
            uuid::Uuid::parse_str(part)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| part.to_string())
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Rows whose target is missing, where the target is a task, page or comment by a type column.
async fn polymorphic_problems(db: &impl Db) -> Result<Vec<Problem>, ApiError> {
    let mut problems = vec![];
    let refs: Vec<(String, String, String)> = crate::db::query(
        db,
        "SELECT lower(hex(file_id)), target_type, lower(hex(target_id)) FROM file_refs
         WHERE (target_type = 'page' AND target_id NOT IN (SELECT id FROM wiki_pages))
            OR (target_type <> 'page' AND target_id NOT IN (SELECT id FROM tasks))",
        [],
    )
    .await?;
    for (file, kind, target) in refs {
        problems.push(Problem {
            table: "file_refs".to_string(),
            id: uuids(&format!("{file}/{kind}/{target}")),
            relation: "target_id -> target".to_string(),
        });
    }
    let revisions: Vec<(String,)> = crate::db::query(db, "SELECT lower(hex(id)) FROM revisions
         WHERE (target_type = 'page' AND target_id NOT IN (SELECT id FROM wiki_pages))
            OR (target_type = 'comment' AND target_id NOT IN (SELECT id FROM comments))
            OR (target_type NOT IN ('page', 'comment') AND target_id NOT IN (SELECT id FROM tasks))", []).await?;
    for (id,) in revisions {
        problems.push(Problem {
            table: "revisions".to_string(),
            id: uuids(&id),
            relation: "target_id -> target".to_string(),
        });
    }
    Ok(problems)
}
