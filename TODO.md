# TODO

## Switch the backend from plain sqlx to SeaORM

Why: we want an ORM (entities, relations, typed filters) without writing one ourselves. `db.rs` was drifting
into one (`create!`, `Update`, `Delete`, `finder!`), and SeaORM covers that ground.

Use the latest version of SeaORM, and stop and report back if that is not possible. Make sure to read updated docs from the [website](https://www.sea-ql.org/SeaORM/docs/index/)

### Decisions already made

- **Migrations first, entities generated.** SeaORM 2.0 (latest stable on crates.io: 2.0.4) supports both
  schema-first (hand-written migrations, then `sea-orm-cli generate entity`) and entity-first
  (`get_schema_registry(..).sync(db)`, features `schema-sync` + `entity-registry`). We use schema-first:
  entity-first sync only adds tables, columns and foreign keys, never drops or rebuilds, cannot add a foreign key
  to an existing SQLite table, and knows nothing about FTS5 tables or triggers. Our migrations are plain SQL
  with table rebuilds, 43 foreign keys, FTS5 and 9 triggers, so the SQL files stay the source of truth.
- **Dates: a jiff newtype (`DeriveValueType`), not chrono (agreed after the spike; see Phase 0).** Old note: sea-query has jiff support in core, but the sqlx integration is still open
  (SeaQL/sea-query#1113, draft #943), so SeaORM entities use chrono for now. One helper in `src/time.rs`
  converts both ways (`From`/`Into` between the chrono value an entity holds and our jiff `time::DateTime`),
  and it is used everywhere. When SeaORM gets jiff, delete the helper and switch the column type.
- **Migrations: SeaORM migrator running the SQL files** (`src/migrations/`), see the end of this section.
- **Raw SQL stays where SQL is the right tool:** search (FTS5), the integrity check (`foreign_key_check`),
  `json_each` filters, `PRAGMA`s, migrations that rebuild tables.
- Work on a new branch off `refactor/sqlx` (commit that first, with the usual message approval).

### Phase 0: spike (done, go)

Scratch crate (not in the repo) with SeaORM 2.0.4 on our own sqlx 0.9 pool and `0001_init.sql`. Results:

- [x] SeaORM 2.0.4 uses sqlx 0.9.0, the same as us, so `SqlxSqliteConnector::from_sqlx_sqlite_pool(pool)` wraps
      our pool: WAL, `foreign_keys(true)`, busy timeout and migrations stay ours. One libsqlite3-sys.
- [x] `Uuid` fields read and write our 16-byte BLOB ids, and rows written by the current sqlx code read back.
- [x] **Dates need no chrono.** `#[derive(DeriveValueType)] #[sea_orm(value_type = "String", from_str, to_str)]`
      on a newtype around `jiff::civil::DateTime` stores `YYYY-MM-DDTHH:MM:SS` text, with typed `lt`/`gt` filters.
      chrono's sqlx encoding would write `YYYY-MM-DD HH:MM:SS` (a space, not `T`), which sorts differently from our
      existing text and would break `created_at < ?` comparisons, so the chrono helper is not the safe route.
- [x] `ActiveEnum` (`rs_type = "String"`, `rename_all = "snake_case"`) matches the lowercase CHECK values.
- [x] `is_in(uuids)`, a raw `Expr::cust_with_values("id IN (SELECT unhex(value) FROM json_each(?))")`,
      `Statement::from_sql_and_values` with `FromQueryResult`, the FTS5 `MATCH` query, `load().with(..)`: all work.
- [x] Foreign keys are enforced through SeaORM (insert with a missing parent fails); transactions begin and roll back.
- [x] Release build of the spike: 198 crates (we have 326 today), 7 MB, 33 s. No arrow compiled.
- [ ] Not yet checked: the app-wide write lock around `begin()`, and the musl/zig build in the real crate.
- [x] **Generated entities are weak on SQLite.** `sea-orm-cli generate entity` (installed 2.0.2, crate is 2.0.4:
      update it) maps BLOB ids to `Vec<u8>`, dates and enums to `String`, and declared type names like `UUID` or
      `DATETIME` come out as `custom(..)` with `ignore`. So entities must be hand-fixed after generation.
- [ ] Decide (see the open decisions in the chat): entities generated once and then hand-maintained with a
      test that compares them to `PRAGMA table_info`, or a generate-and-fix-up task.

### Phase 1: foundations (done)

- [x] `sea-orm` 2.0.4 with only `sqlx-sqlite`, `runtime-tokio`, `macros`, `with-uuid`, `with-json`.
- [x] `time::DateTime` is a SeaORM value type (`DeriveValueType`, text `YYYY-MM-DDTHH:MM:SS`); no chrono, no helper.
- [x] Entities are hand-written in `src/models.rs` (the generator maps blobs and text to `Vec<u8>`/`String`), with a
      test, `entities_match_the_tables`, that compares columns, primary keys and nullability with the migrations.
      No `gen:entities` task and no generation check in CI, because of that.
- [x] `ActiveEnum` for `Role`, `Priority`, `LinkKind`, `Visibility`.
- [x] `DatabaseConnection` over our sqlx pool; `db: &impl Db` everywhere; `Writer` (write lock) derefs to it.

### Phase 2: port by area (done)

- [x] Every area ported: auth, workspaces and members, projects, lists, labels, sprints, saved views, tasks,
      comments, revisions, audit, notifications, wiki pages and live editing, files, trash, import/export,
      calendar, jobs, admin. Search, the integrity check and the task list filters stay raw SQL.
- [x] Entities double as the API types where they did before (`#[schema(as = Name)]`), so the OpenAPI document
      keeps its schema names.

### Phase 3: remove what SeaORM replaces (done)

- [x] `create!`, `Update`, `Delete`, `push_in`, the `Table` trait and the sqlx `FromRow` derives are gone;
      `finder!` was replaced by typed SeaORM queries in a second pass (`db.rs` went from 437 to 120 lines).

### Phase 4: tests, seed, docs, release (done)

- [x] 98 integration tests pass (97 existing plus the entity/table test); `tests/it/db.rs` rewritten.
- [x] The seed binary uses `ActiveModel` inserts plus `create_task_in` and `save_content`; same data as before.
- [x] `AGENTS.md`, `CONTRIBUTING.md` and the backend README updated.
- [x] `mise run check`, `mise run test`, `mise run build:backend`: 20.9 MB (19.0 MB on plain sqlx), 2m11s release
      build (about 1m30s before).
- [ ] Not done: running the app against a database from `main` to confirm the schema is unchanged (the migration
      file is the one sqlx already used; nothing in it changed).

### Third pass (done)

- [x] Migrations moved to `sea-orm-migration`: `src/migrations/` (a `Migrator` plus `m20260101_000001_init.rs`, which
      runs the SQL file beside it); `sqlx::migrate!` and its features are gone. The init migration is raw SQL, not
      schema-builder code, because of CHECK constraints, foreign key actions, FTS5 and triggers. SQLite cannot
      run it atomically, so a failed first run leaves a database to delete.
- [x] Relations and loaders where they reduce code: task labels and assignees (`hydrate_tasks`) and comment authors
      (`load_one`). The other `People` uses stay (trash, notifications, files, audit), because a relation plus a
      `Related` impl each costs more lines than it saves.
- [x] Accepted: id lists use `is_in` (one bind per id, SQLite's variable limit applies).

### Fourth pass (done)

- [x] Sorting, limits and soft-delete filters moved from Rust into the queries (lists, labels, sprints, comments,
      files, notifications, task list including the date range and the sort, siblings as `into_tuple`).
- [x] Counts in SQL: `db::counts` (grouped counts per id) for task comment and attachment counts, label usage and file
      usage/versions; `count(db)` for the unread and admin counts.
- [x] `People` is gone: relations plus `load_one` for files, file versions, notifications, activity and members;
      trash and the assignment events query the users directly.
- Still in Rust on purpose: subtask counting (a tree walk over loaded tasks), sprint summaries (sums over the
  project's loaded tasks), the in-memory wiki sibling list.

### Fifth pass (done)

- [x] The hand-written `src/models.rs` is replaced by the generated entities in `src/models/` (compact format,
      singular `Model` aliases and the enums in hand-edited `mod.rs`/`enums.rs`, jiff dates by an explicit import, API derives,
      the actor relations in `relations.rs`). The migration's column type names (`uuid_text`, `datetime_text`) make the
      generator write `Uuid` and date-time fields. `mise run gen:models` regenerates in place and puts `mod.rs` and the five
      enum columns back; `entities_match_the_tables` still guards the result.
- Module names are the generated plural table names now (`tasks::Entity`, not `task::Entity`).

### Risks to watch

- SeaORM 2.0 is recent; read its changelog before pinning a version.
- jiff in SeaORM's sqlx layer is not released yet; the helper is temporary but used in many places.
- Binary size and compile time can grow with SeaORM on top of sqlx.
- Generated entities and hand-written API structs duplicate field lists; the OpenAPI diff is the safety net.
- Do not push or commit without showing the message and getting approval.
