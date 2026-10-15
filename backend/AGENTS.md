# AGENTS.md

## Commands

- `cargo check` — fast compile check
- `cargo run` — serve on http://localhost:8000 (Swagger UI at `/swagger-ui`)
- `cargo test` — run tests
- `mise run check` — rustfmt --check + clippy (`-D warnings`); run before
  committing
- `mise run fix` — rustfmt + clippy --fix
- `cargo specs` / `mise run specs` — regenerate OpenAPI specs into
  `target/specs/` (openapi.json + openapi.yaml)

## Structure

- Library crate (`src/lib.rs`) with the axum server (`src/main.rs`) and the
  spec generator (`src/bin/specs.rs`) as binaries; imports go through the
  library crate name.

## Conventions

- Rust edition 2024; Toasty requires Rust >= 1.95.
- Models: toasty derives in `src/models.rs`. `#[key] #[auto]` ids are
  generated on insert; `#[unique]`/`#[index]` drive which query methods exist
  (e.g. `get_by_id`, `get_by_email`).
- Toasty query methods need `&mut Db` — acquire with
  `&mut *state.db.lock().await` (tokio mutex in `src/state.rs`).
- Handlers live in `src/routes/`; every handler carries a `#[utoipa::path]`
  annotation. The annotation is the single source of truth — register new
  handlers in `src/routes/mod.rs` via `utoipa_axum::routes!` (group handlers
  that share a path in one `routes!` call) and they appear in both the axum
  router and the OpenAPI docs. Then run `cargo specs` to regenerate
  `target/specs/`.
- `src/openapi.rs` holds only the base document (title/metadata); paths and
  schemas are collected automatically by `routes!`.
- Errors: convert `toasty::Error` to `ApiError` (`src/error.rs`); record-not-
  found maps to 404, everything else to 500.
- Schema: `push_schema` creates tables on startup (quick path). Switch to
  toasty migrations before the schema needs to evolve.

## Phase 2 conventions

- **Soft deletes only.** Projects, lists, tasks, labels, comments and saved views are never destroyed: they get
  `deleted_at`/`deleted_by` (see `src/trash.rs`) and every read path must hide them. The `access::require_*`
  helpers already treat deleted items as not found; use `access::*_workspace` only where Trash needs to see
  them. Cascades stamp children with the parent's `deleted_at`, and restore matches on that timestamp. New
  models that users can delete need the same two columns, a Trash kind in `routes/trash.rs`, and filters.
- **Activity.** After a change succeeds, record it with `activity::record` (best effort; a failure only logs).
  Snapshot titles into `data`, so entries stay readable after renames and deletes.
- **Files** go through `AppState::files` (`storage::FileStores`), never straight to the disk. A file row records
  its `storage_backend` and `storage_key`; reads use the backend that wrote it. Uploads are deduplicated per
  workspace by SHA-256. Files are only hard-deleted by the admin cleanup.
- **Display name is optional** (`User.name` may be empty); the frontend falls back to the email. Display
  preferences (`date_format`, `time_format`) are stored on the user.
- Subtasks live in their parent's list; `trash::subtree` finds a task's descendants among a list's tasks.
- **Inbox.** Every project is created with an `is_inbox` list (first, since list order is id order); it cannot be
  deleted. New list-less quick adds should target it.
- Task listing supports `from`/`to` (a task is scheduled from its start, or due, date to its due, or start,
  date; overlap counts) and `unscheduled`, which the calendar and timeline use.
