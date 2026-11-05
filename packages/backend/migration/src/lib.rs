pub use sea_orm_migration::prelude::*;

mod m20260101_000001_init;
mod m20261103_000001_single_sign_on;
mod m20261105_000001_two_factor;
mod m20261105_000002_api_keys;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260101_000001_init::Migration),
            Box::new(m20261103_000001_single_sign_on::Migration),
            Box::new(m20261105_000001_two_factor::Migration),
            Box::new(m20261105_000002_api_keys::Migration),
        ]
    }
}
