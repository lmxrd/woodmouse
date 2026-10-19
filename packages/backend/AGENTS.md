# AGENTS.md

## Commands

- `cargo check` — fast compile check
- `cargo run` — serve on http://localhost:8000 (Swagger UI at `/swagger-ui`)
- `cargo test` — run tests
- `mise run check` — rustfmt --check + clippy (`-D warnings`); run before
  committing
- `mise run fix` — rustfmt + clippy --fix
- `mise run gen:spec` (or `cargo run --bin specs`) — regenerate OpenAPI specs into
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
- Toasty query methods need an executor. Reads use a per-request handle, `&mut state.db()`; anything that
  writes takes `state.writer().await` (a handle that holds the app-wide write turn, so writers queue in the
  app instead of meeting SQLite's "database is locked"; it derefs to `Db`). Helpers take
  `&mut dyn toasty::Executor`, so they work with a `Db` and with a transaction. There is no global DB mutex.
  Never hold the writer while calling `Rooms` (they take it themselves).
- Shared route helpers (PATCH `double_option`, text validation, downloads) are in `src/api.rs`; `routes::auth::People` loads users by id for "who did it" fields; env settings go through `src/config.rs`.
- Handlers live in `src/routes/`; every handler carries a `#[utoipa::path]`
  annotation. The annotation is the single source of truth — register new
  handlers in `src/routes/mod.rs` via `utoipa_axum::routes!` (group handlers
  that share a path in one `routes!` call) and they appear in both the axum
  router and the OpenAPI docs. Then run `mise run gen:spec` to regenerate
  `target/specs/`.
- `src/openapi.rs` holds only the base document (title/metadata); paths and
  schemas are collected automatically by `routes!`.
- Errors: convert `toasty::Error` to `ApiError` (`src/error.rs`); record-not-
  found maps to 404, everything else to 500.
- Schema: migrations. After changing `src/models.rs` run `cargo run --bin toasty -- generate <name>` and
  commit `backend/toasty/` (history, SQL, snapshot); the server applies pending migrations at startup, and the
  test `migrations_are_up_to_date_with_the_models` fails when you forget. A rename generates drop + create:
  edit the SQL by hand. A dev database made before migrations existed needs `mise run clean`.
  `0000_baseline.sql` is final and never edited again: change the models and generate a new migration. The
  hand-written `0002_search_index.sql` (Toasty cannot describe the FTS5 virtual table or triggers) has a
  history entry pointing at a copy of the previous snapshot, so `generate` still sees no difference, and later
  generated migrations number after it. If a generated migration rebuilds `tasks`, `comments` or
  `wiki_pages`, SQLite drops that table's search triggers: `ensure_triggers` restores them at startup, but
  also add the `CREATE TRIGGER` statements to that migration, and keep `TRIGGERS` in `src/content/search.rs`
  in step.
- Dates: every stored date-time is a `jiff::civil::DateTime` meaning UTC (`src/time.rs`); the API sends RFC 3339
  with `Z` (`#[serde(with = "time::utc")]` + `#[schema(value_type = String, format = DateTime)]`). Task
  `start_date`/`due_date` are a day at 00:00 UTC (`time::to_day`). `time::now()` is whole seconds.
- Relations are `#[belongs_to]` fields for typed navigation only: Toasty enforces them in its query engine, not
  as database constraints. `GET /admin/integrity` and `woodmouse check` report orphaned rows.
- Ordering (tasks, lists, wiki pages): integer `position`s 20 apart (`src/content/ordering.rs`). The API takes
  `after_id` (`null` = first), never a number; neighbours are renumbered only when there is no room.

## Conventions by feature

- **Soft deletes only.** Projects, lists, tasks, labels, comments, saved views, wiki pages and workspaces are
  never destroyed: they get `deleted_at` (see `src/content/trash.rs`) and every read path must hide them. Who deleted
  something is in the audit log, not on the row. The `access::require_*` helpers already treat deleted items as
  not found; use `access::*_workspace` only where Trash needs to see them. Cascades stamp children with the
  parent's `deleted_at`, and restore matches on that timestamp. A deleted workspace is restored by the super
  admin (`/admin/workspaces`) and never purged. The only hard deletes are expired sessions (a daily job) and
  the admin's unused-file cleanup.
- **Audit log** (`src/audit.rs`, `AuditLog`): append-only, written in the same transaction as the change
  (pass the transaction as the executor). `Entry::feed` shows in the workspace activity feed
  (`visibility = workspace`), `Entry::admin` only in the super admin's audit view. `data` is thin (snapshotted
  titles, ids, old and new values), never content; edits carry `revision_id`. No IP addresses or browsers are
  stored. Add the new action's wording to `frontend/src/lib/activity.ts` if it appears in the feed.
- **Revisions** (`src/content/revisions.rs`, `Revision`): one table for earlier versions of tasks, pages and comments;
  a revision holds the new content and the actor. Tasks and comments: every save is a revision. Pages: the
  newest revision is updated in place within an editing session (10 minutes, other editors share it for 2
  minutes); a REST save or restore always starts a new one. Admins can redact a revision's content.
- **Inbox.** A task with `list_id = None` is in its project's inbox; there is no inbox list. `POST
  /projects/{id}/tasks` quick-adds there; `PATCH list_id: null` moves a task back. Subtasks stay with their
  parent (same `project_id` and `list_id`).
- **Completion.** `Task.completed_at` is the only state; `PATCH is_completed: bool` makes the server set or clear
  it. Responses carry `completed_at`; filters still take `is_completed`.
- **Users.** `active` (disabled accounts cannot sign in; sessions are deleted; everything they made stays),
  `global_role` (access to every workspace), `timezone` (the browser's, editable). The super admin's site admin
  routes are in `routes/admin.rs` (users, deleted workspaces, audit log, redaction, integrity check).
- **Login slow-down** (`auth::LoginLimits`): in memory, per email and per address, 1 s doubling up to a cap,
  no lockout; addresses are never stored.
- **Jobs and operations**: `jobs.rs` runs daily (expired sessions) and sends notification email; there is no
  built-in backup, operators copy the database and the files directory themselves. `woodmouse check` runs the
  integrity check. On SIGTERM/Ctrl+C the server saves the open live-editing rooms before exiting, and a clean
  stop checkpoints the write-ahead log so the database file alone is complete.
- **Files** go through `AppState::files` (`storage::FileStores`), never straight to the disk. A file row records
  its `storage_backend` and `storage_key`; reads use the backend that wrote it. Uploads are deduplicated per
  workspace by SHA-256. `FileRef(file_id, target_type, target_id)` attaches a file to a task or page.
- **Display name is optional** (`User.name` may be empty); the frontend falls back to the email. Display
  preferences (`date_format`, `time_format`) are stored on the user.
- Task listing supports `from`/`to` (a task is scheduled from its start, or due, date to its due, or start,
  date; overlap counts) and `unscheduled`, which the calendar and timeline use.
- Wiki: a page (`WikiPage`) holds its blocks as versioned JSON in `content` (`src/content/blocks.rs`, `{"version": 1,
  "blocks": [...]}`; Markdown is only the export format; the editor is only a view). `pages::save_content` is the
  one place that replaces content: it keeps references, the search index and the revision in step. Content saves
  carry `base_version` and bump `WikiPage.version`; a stale save is a 409. Presence ("who else is editing") is in
  memory only (`AppState.presence`). Search is SQLite FTS5 (`src/content/search.rs`): `search_docs` + `search_fts`,
  created by the hand-written migration `0002_search_index.sql` (the triggers are created again at startup by
  `ensure_triggers`), kept current by triggers on `tasks`, comments and page titles plus `index_page_body` on
  content saves. Visibility (workspace, soft delete) is decided at query time
  by joins, so deleting or restoring never touches the index.
- References: `ContentLink` rows (page -> page or task) are rewritten from the blocks inside
  `pages::save_content`, so they can never disagree with the content. `routes/references.rs` serves backlinks
  and `resolve` (current titles, deleted flag).
- Live editing (`src/live/`, `routes/live.rs`): a page opened for editing has a `Room` holding a yrs `Doc`
  (an `XmlFragment` named `default`, laid out as the editor's collaboration extension writes it). Connections
  speak the y-websocket protocol; the hub follows `yrs-warp` (MIT), ported to axum and the current `yrs`.
  `convert.rs` turns the document into blocks (and back, to seed a room); the frontend test
  `yjsFixtures.test.ts` writes `tests/it/fixtures/editor_docs.json` from the real editor schema and
  `tests/it/live_convert.rs` reads it, so the two cannot drift (regenerate with `pnpm vitest run -u
  src/lib/yjsFixtures.test.ts` after changing node types or attributes). ~2s after the last edit the room
  saves through `pages::save_content` (attributed to the last editor); the Yjs state is kept in `PageDoc` with
  an `epoch` that changes whenever the document is rebuilt from blocks (REST save, restore): sockets then close
  with 4001 and clients reload.
- Calendar links (`routes/calendar.rs`, `ical.rs`): `CalendarLink` rows hold a 256-bit secret; the public feed
  (`/api/calendar/<secret>.ics`) re-checks the owner's access on every request and is rebuilt each time. Links are
  revoked (`revoked_at`), never deleted. Task dates are days at 00:00 UTC, so the feed's all-day dates are exact.
  `mode` is `read` only; it is reserved for CalDAV write access later. Handler names must be unique across
  modules (they become client operation ids): hence `list_calendar_links`, not `list_links`.
- Themes (`routes/themes.rs`): workspace palette/accent on `Workspace`, a member's own choice on
  `WorkspaceMember`, light/dark mode on `User.theme_mode`. The palette list is validated here and defined for
  display in the frontend (`themes.ts`); keep them in step.
- **Notifications** (`src/notify.rs`, `routes/notifications.rs`): `notify::notify` writes a `Notification` in the
  transaction of the change (never to the actor; skipped for people who cannot see the workspace; `task_changed`
  and `page_changed` coalesce while unread). Watching is `Watch` rows (`/tasks|pages/{id}/watch`). Email is a
  sweep, not inline: `jobs::send_pending_emails` (every minute) emails notifications for people with
  `email_instant`, `jobs::deadline_notifications` and `jobs::send_digests` run every ten minutes and are idempotent
  (dedupe by due date; digest once per ~20 hours at the person's local hour). Mail goes through the `Mailer`
  trait (`src/mail.rs`): SMTP in production (settings: `EmailSettings` row fields override `SMTP_*`
  env vars), `mail::Outbox` in tests (`AppState::with_mailer`).
- **Files**: `File` describes the newest `FileVersion`; `POST /files/{id}/versions` keeps the old bytes (same id),
  `DELETE /files/{id}` is soft (Trash kind `file`), cleanup removes every version's bytes.
- **Import/export** (`src/transfer/` (`export`, `tasks_csv`, `import`), `routes/transfer.rs`): one plan type (`Export`) for the JSON document and
  for parsed CSV rows; `transfer::apply` previews first (`dry_run`), refuses with the report (422) while there are
  problems or people without a choice (`user_map`: map to a member, create, or skip), and otherwise writes
  everything in one transaction (`create_task_tx` takes the caller's executor), so it is all or nothing. Woodmouse's own schema only; members in a file are reported, never created.
