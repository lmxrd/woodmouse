//! The database: the sqlx pool (WAL, foreign keys on) with SeaORM on top of it for
//! queries. Reads and writes use the entities in [`models`](crate::models); the few statements an ORM does
//! not express (full-text search, the integrity check, `json_each` filters) are raw SQL.
//!
//! Every function that touches the database takes `db: &impl Db`, which a connection, the
//! [`Writer`](crate::state::Writer) and a transaction all are, so a helper works inside or outside a
//! transaction.

use std::str::FromStr;
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, SqlxSqliteConnector, TransactionTrait};
use sea_orm_migration::MigratorTrait;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// What a database function needs: run statements, and start a (nested) transaction.
pub trait Db: ConnectionTrait + TransactionTrait {}
impl<T: ConnectionTrait + TransactionTrait> Db for T {}

/// Open the database at `url` (`sqlite:path`), creating the file when missing. WAL, so reads run beside
/// the one writer.
pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10));
    SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
}

/// SeaORM on the pool.
pub fn orm(pool: SqlitePool) -> DatabaseConnection {
    SqlxSqliteConnector::from_sqlx_sqlite_pool(pool)
}

/// A fresh database in a temporary file, migrated (tests, and anything that needs a throwaway database).
pub async fn temporary() -> DatabaseConnection {
    let path = std::env::temp_dir().join(format!("woodmouse-test-{}.db", uuid::Uuid::new_v4()));
    let db = orm(connect(&format!("sqlite:{}", path.display()))
        .await
        .expect("open a temporary database"));
    migration::Migrator::up(&db, None)
        .await
        .expect("migrate a temporary database");
    db
}

/// The ids as a JSON array of hex strings, for `IN (SELECT unhex(value) FROM json_each(?))`: one bind
/// however many ids (SQLite limits the number of variables), and the query text never changes.
pub fn json_ids(ids: &[uuid::Uuid]) -> String {
    let mut json = String::from("[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push('"');
        json.push_str(&id.simple().to_string());
        json.push('"');
    }
    json.push(']');
    json
}

/// Run a raw statement and read each row as a tuple of columns (by position). For what the entities
/// do not express: `PRAGMA`s and the like.
pub async fn query<T: sea_orm::TryGetableMany>(
    db: &impl ConnectionTrait,
    sql: impl Into<String>,
    values: impl IntoIterator<Item = sea_orm::Value>,
) -> Result<Vec<T>, crate::error::ApiError> {
    let statement =
        sea_orm::Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, sql, values);
    let rows = db.query_all_raw(statement).await?;
    Ok(rows
        .iter()
        .map(T::try_get_many_by_index)
        .collect::<Result<Vec<T>, _>>()
        .map_err(sea_orm::DbErr::from)?)
}

/// How many rows of a query there are per value of `group` (an id column), for the values that have any.
pub async fn counts<E: sea_orm::EntityTrait>(
    db: &impl ConnectionTrait,
    query: sea_orm::Select<E>,
    group: E::Column,
) -> Result<std::collections::HashMap<uuid::Uuid, usize>, crate::error::ApiError> {
    use sea_orm::{
        QuerySelect,
        sea_query::{Expr, Func},
    };
    let rows: Vec<(uuid::Uuid, i64)> = query
        .select_only()
        .column(group)
        .column_as(Expr::expr(Func::count(Expr::col(group))), "count")
        .group_by(group)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|(id, n)| (id, n as usize)).collect())
}

/// `Select::one`, but a missing row is a not-found error (a 404 in a handler).
pub trait OneOrNotFound<E: sea_orm::EntityTrait> {
    fn one_or_not_found(
        self,
        db: &impl ConnectionTrait,
    ) -> impl std::future::Future<Output = Result<E::Model, crate::error::ApiError>>;
}

impl<E: sea_orm::EntityTrait> OneOrNotFound<E> for sea_orm::Select<E> {
    async fn one_or_not_found(
        self,
        db: &impl ConnectionTrait,
    ) -> Result<E::Model, crate::error::ApiError> {
        self.one(db)
            .await?
            .ok_or_else(crate::error::ApiError::not_found)
    }
}

/// The SeaORM traits a database function needs in scope (`find`, `filter`, `Set`, `insert`, `begin`, ...).
pub mod prelude {
    pub use super::OneOrNotFound;
    pub use sea_orm::ActiveValue::{Set, Unchanged};
    pub use sea_orm::sea_query::{Expr, Func};
    pub use sea_orm::{
        ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, LoaderTrait, QueryFilter,
        QueryOrder, QuerySelect, TransactionSession, TransactionTrait,
    };
    pub use sea_orm::{JoinType, Order, PaginatorTrait, QuerySelect as _, RelationTrait};
}
