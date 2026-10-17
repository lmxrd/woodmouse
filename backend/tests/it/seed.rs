//! The seed command fills an empty database, leaves it consistent, and refuses a used one.

#[tokio::test]
async fn seeding_fills_an_empty_database_once() {
    let (_, db) = crate::common::app_with_db().await;
    let state = woodmouse::state::AppState::new(db.clone());
    let router = || {
        woodmouse::routes::api(state.clone())
            .with_state(state.clone())
            .split_for_parts()
            .0
    };
    woodmouse::seed::run(router()).await.unwrap();
    assert!(
        woodmouse::integrity::check(&mut db.clone())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(woodmouse::seed::run(router()).await.is_err());
}
