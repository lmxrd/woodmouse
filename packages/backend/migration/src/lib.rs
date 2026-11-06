pub use sea_orm_migration::prelude::*;

mod m20260101_000001_init;
mod m20261103_000001_single_sign_on;
mod m20261105_000001_two_factor;
mod m20261105_000002_api_keys;
mod m20261106_000001_totp_replace;
mod m20261107_000001_session_store;
mod m20261108_000001_session_generation;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260101_000001_init::Migration),
            Box::new(m20261103_000001_single_sign_on::Migration),
            Box::new(m20261105_000001_two_factor::Migration),
            Box::new(m20261105_000002_api_keys::Migration),
            Box::new(m20261106_000001_totp_replace::Migration),
            Box::new(m20261107_000001_session_store::Migration),
            Box::new(m20261108_000001_session_generation::Migration),
        ]
    }
}
