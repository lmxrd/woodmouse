//! The migrations in `toasty/` must describe the models: after changing `src/models.rs`, run
//! `cargo run --bin toasty -- generate <name>` and commit the result.

#[tokio::test]
async fn migrations_are_up_to_date_with_the_models() {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .unwrap();
    assert!(
        !woodmouse::migrations::is_behind(&db).unwrap(),
        "the models changed: run `cargo run --bin toasty -- generate <name>`"
    );
}

#[tokio::test]
async fn migrations_build_the_schema_the_models_expect() {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let report = woodmouse::migrations::MIGRATIONS.apply(&db).await.unwrap();
    assert!(report.applied() >= 1);
    // Applying again changes nothing.
    let again = woodmouse::migrations::MIGRATIONS.apply(&db).await.unwrap();
    assert_eq!(again.applied(), 0);
    // And the tables answer queries.
    let users = woodmouse::models::User::all()
        .exec(&mut db.clone())
        .await
        .unwrap();
    assert!(users.is_empty());
}
