//! Stopping the server leaves the database file complete: the write-ahead log is emptied.

use woodmouse::state::AppState;

#[tokio::test]
async fn checkpoint_empties_the_write_ahead_log() {
    let dir = std::env::temp_dir().join(format!("woodmouse-wal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let mut db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect(&format!("sqlite:{}", path.display()))
        .await
        .unwrap();
    woodmouse::migrations::MIGRATIONS.apply(&db).await.unwrap();
    toasty::sql::query("PRAGMA journal_mode=WAL")
        .exec(&mut db)
        .await
        .unwrap();
    toasty::create!(woodmouse::models::Workspace {
        name: "Home".to_string(),
        description: "".to_string(),
    })
    .exec(&mut db)
    .await
    .unwrap();

    let wal = dir.join("test.db-wal");
    assert!(
        std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0),
        "writes sit in the log"
    );
    woodmouse::jobs::checkpoint(&AppState::new(db)).await;
    assert_eq!(std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
