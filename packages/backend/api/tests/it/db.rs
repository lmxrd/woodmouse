//! The database layer: typed queries, SeaORM inserts, updates and deletes through the entities, foreign keys, and
//! a check that every entity matches its table.

use api::db::prelude::*;
use api::db::temporary;
use api::time::DateTime;
use entity::Label;
use entity::{labels, workspace_members};
use entity::{users, workspaces};
use sea_orm::DatabaseConnection;

/// A workspace row, so labels can point at something (the relations are foreign keys).
async fn workspace(db: &DatabaseConnection) -> uuid::Uuid {
    workspaces::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        name: Set("Home".to_string()),
        description: Set(String::new()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn label(db: &DatabaseConnection, workspace_id: uuid::Uuid, name: &str) -> Label {
    labels::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        workspace_id: Set(workspace_id),
        name: Set(name.to_string()),
        color: Set("#71717a".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
}

#[tokio::test]
async fn create_returns_the_row_with_a_made_id_and_null_for_what_is_left_out() {
    let pool = temporary().await;
    let db = &pool;
    let ws = workspace(db).await;
    let created = label(db, ws, "Urgent").await;
    assert_eq!(created.workspace_id, ws);
    assert_eq!(created.name, "Urgent");
    assert!(created.deleted_at.is_none());
    // The id is a time-ordered UUID, and the row can be found again.
    assert_eq!(created.id.get_version_num(), 7);
    assert_eq!(
        labels::Entity::find_by_id(created.id)
            .one_or_not_found(db)
            .await
            .unwrap()
            .name,
        "Urgent"
    );
    assert_eq!(
        labels::Entity::find()
            .filter(labels::Column::WorkspaceId.eq(ws))
            .all(db)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_missing_row_is_not_found_and_first_is_none() {
    let pool = temporary().await;
    let db = &pool;
    assert!(
        labels::Entity::find_by_id(uuid::Uuid::now_v7())
            .one_or_not_found(db)
            .await
            .is_err()
    );
    assert!(
        labels::Entity::find()
            .filter(labels::Column::Id.eq(uuid::Uuid::now_v7()))
            .one(db)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn update_sets_only_what_it_is_given_and_an_empty_update_does_nothing() {
    let pool = temporary().await;
    let db = &pool;
    let ws = workspace(db).await;
    let a = label(db, ws, "A").await;
    let b = label(db, ws, "B").await;

    let changed = labels::ActiveModel {
        id: Unchanged(a.id),
        name: Set("Renamed".to_string()),
        ..Default::default()
    }
    .update(db)
    .await
    .unwrap();
    assert_eq!(changed.name, "Renamed");
    let a = labels::Entity::find_by_id(a.id)
        .one_or_not_found(db)
        .await
        .unwrap();
    assert_eq!((a.name.as_str(), a.color.as_str()), ("Renamed", "#71717a"));
    assert_eq!(
        labels::Entity::find_by_id(b.id)
            .one_or_not_found(db)
            .await
            .unwrap()
            .name,
        "B"
    );

    // The filter can come before the set, a value can be cleared to NULL, and nothing set is no change.
    let stamp = DateTime("2026-10-16T08:30:00".parse().unwrap());
    labels::ActiveModel {
        id: Unchanged(b.id),
        deleted_at: Set(Some(stamp)),
        ..Default::default()
    }
    .update(db)
    .await
    .unwrap();
    assert_eq!(
        labels::Entity::find_by_id(b.id)
            .one_or_not_found(db)
            .await
            .unwrap()
            .deleted_at,
        Some(stamp)
    );
    labels::ActiveModel {
        id: Unchanged(b.id),
        deleted_at: Set(None::<DateTime>),
        ..Default::default()
    }
    .update(db)
    .await
    .unwrap();
    assert!(
        labels::Entity::find_by_id(b.id)
            .one_or_not_found(db)
            .await
            .unwrap()
            .deleted_at
            .is_none()
    );
    // Nothing set changes nothing.
    let same = labels::ActiveModel {
        id: Unchanged(b.id),
        ..Default::default()
    }
    .update(db)
    .await
    .unwrap();
    assert_eq!(same.name, "B");
}

#[tokio::test]
async fn delete_and_in_lists_work_with_several_values_and_with_none() {
    let pool = temporary().await;
    let db = &pool;
    let ws = workspace(db).await;
    let a = label(db, ws, "A").await;
    let b = label(db, ws, "B").await;
    let c = label(db, ws, "C").await;

    labels::ActiveModel {
        id: Unchanged(c.id),
        deleted_at: Set(Some(DateTime("2026-10-16T08:30:00".parse().unwrap()))),
        ..Default::default()
    }
    .update(db)
    .await
    .unwrap();
    let ids = [a.id, b.id, c.id];
    let all: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    let live: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::DeletedAt.is_null())
        .filter(labels::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await
        .unwrap();
    assert_eq!(live.len(), 2);
    // An empty list matches nothing (and is valid SQL).
    let none: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::Id.is_in(Vec::<uuid::Uuid>::new()))
        .all(db)
        .await
        .unwrap();
    assert!(none.is_empty());

    let deleted = labels::Entity::delete_many()
        .filter(labels::Column::Id.is_in([a.id, b.id]))
        .exec(db)
        .await
        .unwrap()
        .rows_affected;
    assert_eq!(deleted, 2);
    assert_eq!(labels::Entity::find().all(db).await.unwrap().len(), 1);
    labels::Entity::delete_many()
        .filter(labels::Column::WorkspaceId.eq(ws))
        .exec(db)
        .await
        .unwrap();
    assert!(labels::Entity::find().all(db).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_date_time_round_trips_and_sorts_as_text() {
    let pool = temporary().await;
    let db = &pool;
    let earlier = DateTime("2026-10-16T08:30:00".parse().unwrap());
    let later = DateTime("2026-10-16T08:30:01".parse().unwrap());
    let ws = workspace(db).await;
    for (name, at) in [("late", later), ("early", earlier)] {
        let l = label(db, ws, name).await;
        labels::ActiveModel {
            id: Unchanged(l.id),
            deleted_at: Set(Some(at)),
            ..Default::default()
        }
        .update(db)
        .await
        .unwrap();
    }
    let rows: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::WorkspaceId.eq(ws))
        .order_by_asc(labels::Column::DeletedAt)
        .all(db)
        .await
        .unwrap();
    assert_eq!(rows[0].deleted_at, Some(earlier));
    assert_eq!(rows[1].deleted_at, Some(later));
}

#[tokio::test]
async fn foreign_keys_refuse_orphans_and_protect_parents_and_cascade_join_rows() {
    use entity::Role;
    let pool = temporary().await;
    let db = &pool;

    // A row cannot point at nothing.
    let orphan = labels::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        workspace_id: Set(uuid::Uuid::now_v7()),
        name: Set("x".to_string()),
        color: Set("#000000".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await;
    assert!(orphan.is_err());

    // A parent that still has children cannot be deleted.
    let ws = workspace(db).await;
    label(db, ws, "Urgent").await;
    let delete_workspace = workspaces::Entity::delete_many()
        .filter(workspaces::Column::Id.eq(ws))
        .exec(db)
        .await;
    assert!(delete_workspace.is_err());

    // A join row goes with its parent.
    let user = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set("a@home.io".to_string()),
        name: Set(String::new()),
        password_hash: Set("x".to_string()),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    workspace_members::ActiveModel {
        user_id: Set(user.id),
        workspace_id: Set(ws),
        role: Set(Role::Read),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    assert_eq!(
        workspace_members::Entity::find()
            .all(db)
            .await
            .unwrap()
            .len(),
        1
    );
    users::Entity::delete_many()
        .filter(users::Column::Id.eq(user.id))
        .exec(db)
        .await
        .unwrap();
    assert!(
        workspace_members::Entity::find()
            .all(db)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Every entity names exactly the columns of its table, with the same primary key and nullability.
#[tokio::test]
async fn entities_match_the_tables() {
    use sea_orm::{IdenStatic, Iterable, PrimaryKeyToColumn};
    use std::collections::BTreeSet;

    async fn check<E: EntityTrait>(db: &DatabaseConnection) {
        let table = E::default().table_name().to_string();
        let rows = api::db::query::<(String, i64, i64)>(
            db,
            format!("SELECT name, \"notnull\", pk FROM pragma_table_info('{table}')"),
            [],
        )
        .await
        .unwrap();
        let actual: BTreeSet<(String, bool, bool)> = rows
            .into_iter()
            .map(|(name, notnull, pk)| (name, notnull == 0 && pk == 0, pk > 0))
            .collect();
        let primary: BTreeSet<String> = E::PrimaryKey::iter()
            .map(|k| k.into_column().as_str().to_string())
            .collect();
        let declared: BTreeSet<(String, bool, bool)> = E::Column::iter()
            .map(|c| {
                let name = c.as_str().to_string();
                let pk = primary.contains(&name);
                (name, c.def().is_null(), pk)
            })
            .collect();
        assert_eq!(
            declared, actual,
            "the entity for `{table}` differs from the table"
        );
    }

    let db = temporary().await;
    macro_rules! all {
        ($($m:ident),+) => { $( check::<entity::$m::Entity>(&db).await; )+ };
    }
    all!(
        audit_logs,
        calendar_links,
        comments,
        content_links,
        email_settings,
        file_refs,
        file_versions,
        files,
        labels,
        lists,
        notification_settings,
        notifications,
        page_docs,
        projects,
        revisions,
        saved_views,
        sessions,
        sprints,
        task_assignees,
        task_labels,
        task_links,
        tasks,
        user_invites,
        users,
        watches,
        wiki_pages,
        workspace_members,
        workspaces
    );
}
