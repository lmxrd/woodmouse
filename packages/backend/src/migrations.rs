//! The database schema's history. Migrations are SQL files generated from the models by
//! `cargo run --bin toasty -- generate <name>` into `toasty/` (the history, one SQL file and one schema
//! snapshot per migration) and embedded into the binary; the server applies the pending ones at startup.

use std::path::PathBuf;

use toasty::migration::{self, History, HistoryEntry, MigrationSet, Snapshot};
use toasty::schema::{db, diff};

/// Every migration, in order.
pub static MIGRATIONS: MigrationSet = toasty::embed_migrations!();

/// Where the history, migrations and snapshots live in the source tree.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("toasty")
}

/// What the models need that the last snapshot does not have yet, with the history it would extend.
fn pending(
    db: &toasty::Db,
) -> Result<(History, Option<migration::Generated>), Box<dyn std::error::Error>> {
    let root = dir();
    let history = History::load_or_default(root.join("history.toml"))?;
    let previous = match history.entries().last() {
        Some(entry) => Snapshot::load(root.join("snapshots").join(&entry.snapshot_name))?.schema,
        None => db::Schema::default(),
    };
    let generated = migration::generate(
        db.driver(),
        &previous,
        &db.schema().db,
        &diff::RenameHints::default(),
    );
    Ok((history, generated))
}

/// Whether the models differ from the last snapshot, so that a migration needs generating.
pub fn is_behind(db: &toasty::Db) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(pending(db)?.1.is_some())
}

/// Compare the models with the last snapshot and, when they differ, write the next migration (SQL file,
/// snapshot and history entry). Returns the new migration's file name, or `None` when nothing changed.
/// A renamed table or column shows up as a drop and a create: edit the generated SQL by hand to keep the
/// data (an `ALTER TABLE ... RENAME`), before it is ever applied anywhere.
pub fn generate(db: &toasty::Db, name: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let (mut history, generated) = pending(db)?;
    let Some(generated) = generated else {
        return Ok(None);
    };
    let root = dir();
    let db::Migration::Sql(sql) = generated.migration;

    let number = history.next_migration_number();
    let sql_name = format!("{number:04}_{name}.sql");
    let snapshot_name = format!("{number:04}_snapshot.toml");
    std::fs::create_dir_all(root.join("migrations"))?;
    std::fs::create_dir_all(root.join("snapshots"))?;
    std::fs::write(root.join("migrations").join(&sql_name), sql)?;
    generated
        .snapshot
        .save(root.join("snapshots").join(&snapshot_name))?;
    history.add_entry(HistoryEntry {
        id: uuid::Uuid::new_v4().as_u64_pair().0,
        name: sql_name.clone(),
        snapshot_name,
        checksum: None,
    });
    history.save(root.join("history.toml"))?;
    Ok(Some(sql_name))
}
