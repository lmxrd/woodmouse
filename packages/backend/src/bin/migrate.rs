//! `mise run db:migrate` (or `cargo run --bin migrate`): apply the pending migrations to the database in
//! `DATABASE_URL` (default `sqlite:woodmouse.db`) and restore the search triggers, then exit. The server does the
//! same when it starts; this is for doing it on its own, without starting the server or the file storage.

use sea_orm_migration::MigratorTrait;
use woodmouse::{db, migrations::Migrator, search};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:woodmouse.db".to_string());
    let database = db::orm(db::connect(&url).await?);
    Migrator::up(&database, None).await?;
    search::ensure_triggers(&database)
        .await
        .map_err(|_| "could not create the search triggers")?;
    println!("the database is up to date");
    Ok(())
}
