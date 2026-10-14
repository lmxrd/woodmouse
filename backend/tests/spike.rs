//! toasty spike (PLAN.md §3): prove the storage assumptions before any
//! Phase 0 store code. The three claims under test:
//!
//! (a) FTS5 virtual tables work alongside toasty-managed tables
//! (b) recursive page-tree queries (SQLite recursive CTE) run fine
//! (c) raw SQL executes on the *same* pool / database as toasty models
//!
//! Plus: WAL journal mode persists across pool connections.
//! Results are recorded in PLAN.md "Open items".

use toasty::stmt::Value;

/// Minimal wiki-page shape: nullable self-reference via `parent_id`.
#[derive(Debug, toasty::Model)]
struct SpikePage {
    #[key]
    #[auto]
    id: uuid::Uuid,
    parent_id: Option<uuid::Uuid>,
    title: String,
}

async fn spike_db() -> toasty::Db {
    let db = toasty::Db::builder()
        .models(toasty::models!(crate::*))
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    db.push_schema().await.expect("push schema");
    db
}

fn string_at(row: &Value, col: usize) -> String {
    match row {
        Value::Record(rec) => match &rec[col] {
            Value::String(s) => s.clone(),
            other => panic!("expected string at column {col}, got {other:?}"),
        },
        other => panic!("expected record row, got {other:?}"),
    }
}

fn i64_at(row: &Value, col: usize) -> i64 {
    match row {
        Value::Record(rec) => match &rec[col] {
            Value::I64(n) => *n,
            other => panic!("expected i64 at column {col}, got {other:?}"),
        },
        other => panic!("expected record row, got {other:?}"),
    }
}

#[tokio::test]
async fn fts5_recursive_cte_and_raw_sql_share_the_toasty_pool() {
    let mut db = spike_db().await;

    // Page tree: root <- child <- grandchild (the wiki shape).
    let root = toasty::create!(SpikePage {
        parent_id: None,
        title: "Root page".to_string(),
    })
    .exec(&mut db)
    .await
    .expect("create root");
    let child = toasty::create!(SpikePage {
        parent_id: Some(root.id),
        title: "Child about rust".to_string(),
    })
    .exec(&mut db)
    .await
    .expect("create child");
    let _grandchild = toasty::create!(SpikePage {
        parent_id: Some(child.id),
        title: "Grandchild notes".to_string(),
    })
    .exec(&mut db)
    .await
    .expect("create grandchild");

    // (c) raw SQL reads rows toasty wrote: same database, same pool.
    let rows = toasty::sql::query("SELECT title FROM spike_pages WHERE parent_id IS NOT NULL")
        .exec(&mut db)
        .await
        .expect("raw select");
    assert_eq!(rows.len(), 2, "toasty-created rows visible to raw SQL");

    // (c) the other direction: raw SQL writes, toasty reads. Note: toasty
    // stores UUIDs as 16-byte BLOBs on SQLite, and raw-SQL binds must use
    // that storage representation — `bind(uuid)` panics in the sqlite
    // driver, and the string form does not match.
    let raw_id = uuid::Uuid::new_v4();
    let inserted = toasty::sql::statement(
        "INSERT INTO spike_pages (id, parent_id, title) VALUES (?1, ?2, ?3)",
    )
    .bind(raw_id.as_bytes().to_vec())
    .bind(root.id.as_bytes().to_vec())
    .bind("Raw SQL page")
    .exec(&mut db)
    .await
    .expect("raw insert");
    assert_eq!(inserted, 1);
    let read_back = SpikePage::get_by_id(&mut db, &raw_id)
        .await
        .expect("read raw-SQL row via toasty");
    assert_eq!(read_back.title, "Raw SQL page");

    // (a) FTS5 virtual table next to toasty-managed tables, fed from them.
    // Contentless index (content=''): MATCH finds the rowid, the page title
    // comes from a join back to the source table.
    toasty::sql::statement("CREATE VIRTUAL TABLE spike_fts USING fts5(title, content='')")
        .exec(&mut db)
        .await
        .expect("create fts5 table");
    toasty::sql::statement(
        "INSERT INTO spike_fts (rowid, title) SELECT rowid, title FROM spike_pages",
    )
    .exec(&mut db)
    .await
    .expect("seed fts5 table");
    let hits = toasty::sql::query(
        "SELECT p.title FROM spike_fts \
         JOIN spike_pages p ON p.rowid = spike_fts.rowid \
         WHERE spike_fts MATCH ?1",
    )
    .bind("rust")
    .exec(&mut db)
    .await
    .expect("fts5 match query");
    assert_eq!(hits.len(), 1, "exactly one page mentions rust");
    assert_eq!(string_at(&hits[0], 0), "Child about rust");

    // (b) recursive page-tree query: all descendants of root (3 children now,
    // including the raw-SQL-inserted page).
    let rows = toasty::sql::query(
        "WITH RECURSIVE tree(id) AS ( \
             SELECT ?1 \
             UNION ALL \
             SELECT p.id FROM spike_pages p JOIN tree t ON p.parent_id = t.id \
         ) SELECT COUNT(*) FROM tree",
    )
    .bind(root.id.as_bytes().to_vec())
    .exec(&mut db)
    .await
    .expect("recursive cte");
    assert_eq!(
        i64_at(&rows[0], 0),
        4,
        "root + child + grandchild + raw page"
    );
}

#[tokio::test]
async fn wal_journal_mode_persists_across_pool_connections() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("woodmouse-spike-{}.db", uuid::Uuid::new_v4()));
    let db_url = format!("sqlite:{}", path.display());

    let mut db = toasty::Db::builder()
        .models(toasty::models!(crate::*))
        .connect(&db_url)
        .await
        .expect("connect file db");
    db.push_schema().await.expect("push schema");

    // Set WAL once (startup pattern); it is a persistent property of the file.
    let mode = toasty::sql::query("PRAGMA journal_mode=WAL")
        .exec(&mut db)
        .await
        .expect("set wal");
    assert_eq!(string_at(&mode[0], 0), "wal");

    // A freshly acquired pool connection sees the same mode.
    let mut conn = db.connection().await.expect("fresh connection");
    let mode = toasty::sql::query("PRAGMA journal_mode")
        .exec(&mut conn)
        .await
        .expect("read journal mode");
    assert_eq!(string_at(&mode[0], 0), "wal");

    drop(db);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}
