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

- Wiki: a page is an ordered list of blocks in the app's own JSON format (`src/blocks.rs`; the editor is only a
  view). Content saves carry `base_version` and bump `Page.version`; a stale save is a 409. Presence ("who else
  is editing") is in memory only (`AppState.presence`). Search is SQLite FTS5 (`src/search.rs`):
  `search_docs` + `search_fts`, created by `ensure_index` at startup and in test setup, kept current by triggers
  on `tasks` and page titles plus `index_page_body` on block saves. Visibility (workspace, soft delete) is
  decided at query time by joins, so deleting or restoring never touches the index.
- Version history (`src/revisions.rs`, `routes/history.rs`): a version is a full copy of what was there before
  an edit that starts a new session (different editor, 10-minute pause, hourly in long sessions); restores force
  a version of the current content first. Never deleted. Markdown export is `src/markdown.rs` plus
  `routes/export.rs` (zip via the `zip` crate).
- References: `ContentLink` rows (page -> page or task) are rewritten from the blocks inside `write_blocks`, so
  they can never disagree with the content. `routes/references.rs` serves backlinks and `resolve` (current
  titles, deleted flag); links in content store ids, the label is a display fallback only.
- Live editing (`src/live/`, `routes/live.rs`): a page opened for editing has a `Room` holding a yrs `Doc`
  (an `XmlFragment` named `default`, laid out as the editor's collaboration extension writes it). Connections
  speak the y-websocket protocol; the hub follows `yrs-warp` (MIT), ported to axum and the current `yrs`.
  `convert.rs` turns the document into blocks (and back, to seed a room); the frontend test
  `yjsFixtures.test.ts` writes `tests/fixtures/editor_docs.json` from the real editor schema and
  `tests/live_convert.rs` reads it, so the two cannot drift (regenerate with `pnpm vitest run -u
  src/lib/yjsFixtures.test.ts` after changing node types or attributes). ~2s after the last edit the room
  saves through `write_blocks` (attributed to the last editor); the Yjs state is kept in `PageDoc` with an
  `epoch` that changes whenever the document is rebuilt from blocks (REST save, restore): sockets then close
  with 4001 and clients reload. Never hold the db lock while calling `Rooms` (rooms take the db lock inside).
- Calendar links (`routes/calendar.rs`, `ical.rs`): `CalendarLink` rows hold a 256-bit secret; the public feed
  (`/api/calendar/<secret>.ics`) re-checks the owner's access on every request and is rebuilt each time. Links are
  revoked (`revoked_at`), never deleted. Task dates are stored as local midnight in the saver's timezone, so
  `ical::date_of` takes the nearest UTC midnight (right for timezones within 12h of UTC). `mode` is `read` only;
  it is reserved for CalDAV write access later. Handler names must be unique across modules (they become client
  operation ids): hence `list_calendar_links`, not `list_links`.
