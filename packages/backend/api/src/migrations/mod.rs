//! The schema, as SeaORM migrations (run at startup, each once, recorded in `seaql_migrations`). Each migration
//! is a Rust file that runs its SQL file beside it: plain SQL, because the schema needs what the schema builder
//! does not express (CHECK constraints, foreign key actions, the FTS5 table and its triggers, table rebuilds).
//! Add a migration for every schema change, in order, and edit none that has shipped.

use sea_orm_migration::prelude::*;

mod m20260101_000001_init;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260101_000001_init::Migration)]
    }
}
