use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::DatabaseExecutor;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // `PRAGMA foreign_keys` has no effect inside a transaction and is per connection, while SeaORM
        // runs statements on whichever pooled connection is free. So the rebuild takes one connection,
        // turns the pragma off, runs in its own transaction there, and turns it back on.
        let DatabaseExecutor::Connection(db) = manager.get_connection() else {
            return Err(DbErr::Migration(
                "the single sign-on migration must run outside a transaction".into(),
            ));
        };
        let mut conn = db
            .get_sqlite_connection_pool()
            .acquire()
            .await
            .map_err(sqlx_err)?;
        // Closed rather than returned to the pool, however this ends (an error, or startup cancelled
        // halfway): a pooled connection must never keep foreign keys off or a transaction open.
        conn.close_on_drop();
        let result = rebuild(&mut conn).await;
        if result.is_err() {
            let _ = sqlx::raw_sql("ROLLBACK").execute(&mut *conn).await;
        }
        let _ = sqlx::raw_sql("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        result?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration("not reversible".into()))
    }
}

async fn rebuild(conn: &mut sqlx::SqliteConnection) -> Result<(), DbErr> {
    sqlx::raw_sql("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .map_err(sqlx_err)?;
    sqlx::raw_sql("BEGIN IMMEDIATE")
        .execute(&mut *conn)
        .await
        .map_err(sqlx_err)?;
    sqlx::raw_sql(include_str!("m20261103_000001_single_sign_on.sql"))
        .execute(&mut *conn)
        .await
        .map_err(sqlx_err)?;
    let broken: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut *conn)
            .await
            .map_err(sqlx_err)?;
    if !broken.is_empty() {
        let tables: Vec<_> = broken.into_iter().map(|(t,)| t).collect();
        return Err(DbErr::Migration(format!(
            "rows in {} point at nothing (run `woodmouse check` on a copy to see them); \
             the users rebuild was not applied",
            tables.join(", ")
        )));
    }
    sqlx::raw_sql("COMMIT")
        .execute(&mut *conn)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

fn sqlx_err(e: sqlx::Error) -> DbErr {
    DbErr::Migration(e.to_string())
}
