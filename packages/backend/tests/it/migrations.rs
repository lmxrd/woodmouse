//! The migrations in `src/migrations/` build a working database, search index included.
use sea_orm_migration::MigratorTrait;
use woodmouse::db::prelude::*;
use woodmouse::models::users;

fn names(rows: Vec<(String,)>) -> Vec<String> {
    let mut names: Vec<String> = rows.into_iter().map(|(name,)| name).collect();
    names.sort();
    names
}

#[tokio::test]
async fn migrations_build_the_schema_the_models_expect() {
    let db = woodmouse::db::temporary().await;
    // Applying again changes nothing.
    woodmouse::migrations::Migrator::up(&db, None)
        .await
        .unwrap();
    // And the tables answer queries.
    let users = users::Entity::find().all(&db).await.unwrap();
    assert!(users.is_empty());
}

#[tokio::test]
async fn migrations_create_the_search_index_and_its_triggers() {
    let db = woodmouse::db::temporary().await;
    let pool = db.get_sqlite_connection_pool().clone();
    let triggers: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'search%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(names(triggers).len(), 9);
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'search_fts'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(names(tables), vec!["search_fts".to_string()]);
}

#[tokio::test]
async fn startup_triggers_match_the_migration() {
    // What the migrations made.
    let db = woodmouse::db::temporary().await;
    let pool = db.get_sqlite_connection_pool().clone();
    let by_name = |rows: Vec<(String, String)>| {
        let mut rows: Vec<(String, String)> = rows
            .into_iter()
            .map(|(name, sql)| (name, sql.split_whitespace().collect::<Vec<_>>().join(" ")))
            .collect();
        rows.sort();
        rows
    };
    let query =
        "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'search%'";
    let from_migration = by_name(sqlx::query_as(query).fetch_all(&pool).await.unwrap());
    assert_eq!(from_migration.len(), 9);

    // Lose them all (as a rebuilt table would), and the startup step brings every one back, identical.
    for (name, _) in &from_migration {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TRIGGER {name}")))
            .execute(&pool)
            .await
            .unwrap();
    }
    woodmouse::search::ensure_triggers(&db).await.unwrap();
    let restored = by_name(sqlx::query_as(query).fetch_all(&pool).await.unwrap());
    assert_eq!(restored, from_migration);
}
