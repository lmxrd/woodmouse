//! Migrations that rebuild a table, run on a real database file that already has rows in it.

use api::db::prelude::*;
use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;

const SSO_MIGRATION: &str = "m20261103_000001_single_sign_on";

async fn open(path: &std::path::Path) -> DatabaseConnection {
    api::db::orm(
        api::db::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap(),
    )
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    api::db::query::<(i64,)>(db, sql, []).await.unwrap()[0].0
}

/// The rows the rebuild must keep: users with and without a password, their sessions, a membership and
/// an invite (all of which cascade when a user is deleted).
async fn seed_before_sso(db: &DatabaseConnection) -> (uuid::Uuid, uuid::Uuid) {
    let with_password = uuid::Uuid::now_v7();
    let invited = uuid::Uuid::now_v7();
    let workspace = uuid::Uuid::now_v7();
    // Raw SQL: the entities describe the schema after the migration.
    for (sql, values) in [
        (
            "INSERT INTO users (id, email, name, password_hash, super_admin, active) VALUES (?, ?, '', ?, 1, 1)",
            vec![
                with_password.into(),
                "admin@home.io".into(),
                "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaA".into(),
            ],
        ),
        (
            "INSERT INTO users (id, email, name, password_hash, super_admin, active) VALUES (?, ?, '', '', 0, 1)",
            vec![invited.into(), "invited@home.io".into()],
        ),
        (
            "INSERT INTO sessions (id, token, user_id, expires_at) VALUES (?, 'token-1', ?, '2100-01-01T00:00:00')",
            vec![uuid::Uuid::now_v7().into(), with_password.into()],
        ),
        (
            "INSERT INTO workspaces (id, name, description) VALUES (?, 'Home', '')",
            vec![workspace.into()],
        ),
        (
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES (?, ?, 'write')",
            vec![workspace.into(), invited.into()],
        ),
        (
            "INSERT INTO user_invites (id, user_id, token, expires_at) VALUES (?, ?, 'invite-1', '2100-01-01T00:00:00')",
            vec![uuid::Uuid::now_v7().into(), invited.into()],
        ),
    ] {
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
            values,
        ))
        .await
        .unwrap();
    }
    (with_password, invited)
}

async fn assert_kept(db: &DatabaseConnection, with_password: uuid::Uuid, invited: uuid::Uuid) {
    assert_eq!(count(db, "SELECT count(*) FROM users").await, 2);
    assert_eq!(count(db, "SELECT count(*) FROM sessions").await, 1);
    assert_eq!(count(db, "SELECT count(*) FROM workspace_members").await, 1);
    assert_eq!(count(db, "SELECT count(*) FROM user_invites").await, 1);
    assert_eq!(
        count(db, "SELECT count(*) FROM pragma_foreign_key_check").await,
        0
    );
    // Raw queries: this runs before later migrations add columns the entity has.
    let password_of = |id: uuid::Uuid| {
        format!(
            "SELECT count(*) FROM users WHERE id = x'{}' AND {}",
            id.simple(),
            "password_hash"
        )
    };
    assert_eq!(
        count(
            db,
            &format!("{} LIKE '$argon2id$%'", password_of(with_password))
        )
        .await,
        1
    );
    // The empty string that meant "no password" is NULL now.
    assert_eq!(
        count(db, &format!("{} IS NULL", password_of(invited))).await,
        1
    );
    // Every pooled connection still enforces foreign keys.
    let pool = db.get_sqlite_connection_pool();
    let mut held = vec![];
    for _ in 0..8 {
        let mut conn = pool.acquire().await.unwrap();
        let (on,): (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(on, 1, "a pooled connection has foreign keys off");
        held.push(conn);
    }
    // And they cascade again.
    drop(held);
    entity::users::Entity::delete_by_id(invited)
        .exec(db)
        .await
        .unwrap();
    assert_eq!(count(db, "SELECT count(*) FROM workspace_members").await, 0);
}

/// `password_hash` becomes nullable by rebuilding `users`, which must not cascade into sessions,
/// memberships and invites, and must survive being run again (the migration is recorded only after it
/// ran).
#[tokio::test]
async fn the_users_rebuild_keeps_every_row_and_can_run_again() {
    let path =
        std::env::temp_dir().join(format!("woodmouse-migration-{}.db", uuid::Uuid::new_v4()));
    let db = open(&path).await;
    migration::Migrator::up(&db, Some(1)).await.unwrap();
    let (with_password, invited) = seed_before_sso(&db).await;

    // Only the single sign-on migration: a later one replaces the sessions table.
    migration::Migrator::up(&db, Some(1)).await.unwrap();
    assert_kept(&db, with_password, invited).await;
    drop(db);

    // A crash after the migration ran but before it was recorded: it runs again on the next start.
    let db = open(&path).await;
    // Put the invitee back (deleted above to prove the cascade).
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, email, name, password_hash, super_admin, active) VALUES (x'{}', 'invited@home.io', '', NULL, 0, 1)",
        invited.simple()
    ))
    .await
    .unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO user_invites (id, user_id, token, expires_at) VALUES (x'{}', x'{}', 'invite-1', '2100-01-01T00:00:00')",
        uuid::Uuid::now_v7().simple(),
        invited.simple()
    ))
    .await
    .unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) SELECT id, x'{}', 'write' FROM workspaces",
        invited.simple()
    ))
    .await
    .unwrap();
    db.execute_unprepared(&format!(
        "DELETE FROM seaql_migrations WHERE version = '{SSO_MIGRATION}'"
    ))
    .await
    .unwrap();
    drop(db);
    let db = open(&path).await;
    migration::Migrator::up(&db, Some(1)).await.unwrap();
    assert_kept(&db, with_password, invited).await;

    // The session store's migration signs everyone out once: the old sessions go, and the new table is
    // keyed by a hash of the session id.
    migration::Migrator::up(&db, None).await.unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM sessions").await, 0);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM pragma_table_info('sessions') WHERE name IN ('id_hash', 'data', 'expires_at')"
        )
        .await,
        3
    );
    let _ = std::fs::remove_file(&path);
}
