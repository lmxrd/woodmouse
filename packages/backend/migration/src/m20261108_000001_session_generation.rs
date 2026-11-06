use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

/// The session generation on users (see the `.sql`).
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // SQLite has no `ADD COLUMN IF NOT EXISTS`, and a migration that crashed before it was recorded
        // runs again: add the column only when it is missing.
        let present = db
            .query_one_raw(Statement::from_string(
                db.get_database_backend(),
                "SELECT 1 FROM pragma_table_info('users') WHERE name = 'session_generation'",
            ))
            .await?
            .is_some();
        if !present {
            db.execute_unprepared(include_str!("m20261108_000001_session_generation.sql"))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration("not reversible".into()))
    }
}
