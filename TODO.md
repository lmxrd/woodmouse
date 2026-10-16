# TODO

## Conventions decided for this round

- **Dates and times**: every date/time field is a `jiff::civil::DateTime` that always means **UTC**. The API sends them as ISO 8601 with a `Z` suffix so no client can read them as local time. Task `start_date` and `due_date` are a calendar day stored as that day at `00:00` UTC; only the date part counts, so a due date never shifts with anyone's timezone (and the calendar feed no longer has to guess the day).
- **Ordering**: tasks, lists and wiki pages use integer positions spaced **20** apart. A new item goes last (max + 20); a move takes the midpoint of its neighbours. When there is no whole number left between them, renumber that list (or that page's siblings) to 20, 40, 60, ... Blocks need no positions: they are an ordered list inside the page's JSON content.
- **References**: declare Toasty relations (`#[belongs_to]`, `#[has_many]`, `#[has_one]`) for typed navigation and `.include()` preloading. Toasty enforces them in its query engine, not with database constraints, and its automatic cascades only run on real deletes (we soft-delete almost everything), so check which hard deletes would cascade. No hand-written `FOREIGN KEY` constraints.
- **One database handle per request**: replace the single `AppState.db` mutex with a handle on the connection pool per request, using transactions where writes must happen together. SQLite in WAL mode then serves reads in parallel; writes still take turns. Do this in the same pass as the model changes, since every handler is touched anyway. (The live-editing rooms take the database inside; keep them free of nested locking.)

## Users and access

- [x] **User model**: Add `active: bool` (or `is_disabled: bool`) field to `User` struct in `backend/src/models.rs` so admins can enable/disable user accounts. Disabling blocks access but keeps everything they made: login is refused, their sessions are deleted, live-editing connections close, calendar links stop answering, and they can't be newly assigned or invited. Memberships, tasks, comments and pages stay, shown with their name marked as disabled. Re-enabling restores access as it was. Record enable/disable in the `AuditLog`.

- [x] **User model**: Keep `super_admin: bool` as a distinct tier above `Role::Admin` (gates the site-wide admin page, only one user, never delegatable), and add `global_role: Option<Role>` for delegated cross-workspace access (`Some(Read)`, `Some(Write)` or `Some(Admin)` — all distinct from super admin; `None` means workspace memberships only). Effective workspace role = max(super_admin → Admin, global_role, workspace role). Update `PublicUser` DTO and admin UI accordingly. People with a `global_role` see every workspace in the workspace list.

- [x] **User model**: Add `timezone: Option<String>` (IANA name, e.g. `Europe/Oslo`). Fill it in from the browser the first time the person signs in, editable on the account page. The server uses it to decide "today" for the person and to send digests at their chosen local time.

- [x] **Invite model**: Rename `Invite` struct to `UserInvite` in `backend/src/models.rs` (and update all references).

- [x] **Login limits**: Slow down repeated failed logins: after 5 failures for one email, or from one network address, within 15 minutes, further attempts wait with growing delays. Kept in memory only (the address is never stored), no account lockout (nobody can lock someone else out). Failures are still recorded in the `AuditLog` (without IP).

- [x] **Sessions**: Delete expired sessions for real with a daily background job (sessions are login tokens, not user content, so the soft-delete rule does not apply). Logouts and revocations stay in the `AuditLog`.

## Workspaces and projects

- [x] **Workspace/Project models**: Add a `description: String` field to both `Workspace` and `Project` structs in `backend/src/models.rs`.

- [x] **Workspace deletion** (new): Admins can delete a workspace. Soft delete only, never purged: set `deleted_at`, hide it for everyone, list it in a site-wide trash only the super admin sees, where it can be restored. Its projects, pages and files stay in the database (files only ever go with their workspace, so they are kept too). Recorded in the `AuditLog`.

## Audit log and history

- [x] **AuditLog (new model, merges Activity)**: Add an append-only `AuditLog` table in `backend/src/models.rs` that absorbs the `Activity` feed — drop the `Activity` table. Immutable: no update, delete, or prune paths; write the entry in the same transaction as the mutation. Fields: `id` (auto UUID), `actor_id: Option<uuid::Uuid>` (indexed; `None` for anonymous events), `action: String` (indexed, snake_case: `login`, `login_failed`, `logout`, `password_changed`, `role_changed`, `user_disabled`, `entity_deleted`, `entity_restored`, `session_revoked`, `task_created`, `comment_added`, `task_edited`, `page_edited`, `comment_edited`, ...), `target_type: Option<String>` (`user`, `workspace`, `session`, `task`, `page`, `comment`, ...), `target_id: Option<uuid::Uuid>` (indexed), `workspace_id: Option<uuid::Uuid>` (indexed; workspace/feed events have one, site-wide events like logins don't), `visibility: Visibility` (new enum, snake_case like `Role`: `workspace` = shown in the workspace activity feed and the audit; `admin` = audit only), `data: String` (JSON: old → new values for privilege changes, snapshotted titles for feed phrasing and deleted entities, and for edits a pointer to the `Revision` (`revision_id`); thin records only, never content payloads — content lives in `Revision`), `created_at`. **No `ip` or `user_agent` fields**: the log records who did what and when, not where from. Rules: `visibility = workspace` implies `workspace_id` is set; feed query = `WHERE visibility = workspace AND workspace_id = ?`; the feed endpoint maps rows to a view with only `id`, `actor_id`, `action`, `data`, `created_at` (Toasty loads whole rows, so the whitelist lives in the DTO); only the super admin reads the full log (site-wide admin page). Records: authentication events (`admin`), privilege changes (role, `global_role`, disable/enable), destructive actions (soft deletes, restores, workspace deletions), and all former `Activity` feed events (`workspace`). Add a composite index for the feed query (`workspace_id`, `visibility`, `created_at`).

- [x] **Revision (new model, replaces TaskRevision and PageRevision)**: One generic table for earlier versions of tasks, wiki pages and comments: `id`, `target_type` (`task`, `page` or `comment`), `target_id` (indexed), `actor_id`, `created_at`, `updated_at`, `content` (JSON: title and description for tasks, title and blocks for pages, body for comments). A revision holds the **new** version after a change and who made it. Tasks and comments save rarely, so each save is a revision. Pages autosave every few seconds, so a page's newest revision is updated in place until the editing session ends (another editor, or a 10-minute pause). Restoring writes a new revision. The `AuditLog` entry for an edit points to its revision. Admins can redact a revision's content if something sensitive was saved (the audit entry stays). The history panel works the same for all three types.

- [x] **Models (all)**: Drop `deleted_by: Option<uuid::Uuid>` from all models (`Project`, `List`, `Task`, `Label`, `Comment`, `SavedView`, `Page`) — the `AuditLog` records the actor of every deletion. Keep `deleted_at` (queries filter on it); Trash UI looks up "deleted by" from `AuditLog` per item. Items deleted along with their parent (a project's lists and tasks share its `deleted_at`) only have an entry for the parent, so the Trash shows the parent's entry for them.

## Tasks

- [x] **Task model**: Add `project_id: uuid::Uuid` (indexed) to `Task` — the task's project directly, instead of only through `list_id -> List.project_id`. Make `list_id: Option<uuid::Uuid>` where `None` means the task is in the project's inbox (a view, not a list row). Drop `List.is_inbox` and the seeded inbox list (no "exactly one per project" / "cannot be deleted" special cases). Maintain the code-level invariant in create/move paths: when `list_id` is set, `list.project_id == task.project_id`. "Move to inbox" = set `list_id = None`; "triage" = set a list. Subtasks always live with their parent: same project and same list (or none). Moving a parent moves its whole subtree, and a subtask can't be moved to another list on its own. The URL filter and saved views need a way to say "inbox" for `list_id`.

- [x] **Task model**: Remove `is_completed: bool` — completion state is derived from `completed_at` (`Some` = completed, `None` = not). Responses carry `completed_at` (read-only). To close or reopen a task, `PATCH` sends `is_completed: true/false` and the server sets `completed_at` to now or clears it; `completed_at` can't be set through the API. Update all code and clients accordingly.

- [x] **Task model**: Change `position: f64` to `i64` with gaps of 20 (see Conventions). Same for `List.position` and `Page.position`.

- [x] **Task model**: Remove `text_edited_at: Option<i64>` and `text_edited_by: Option<uuid::Uuid>` — "last edited by/at" is the newest `Revision` of the task (revisions now hold the new version and its actor, see `Revision`). The session rule for revisions reads the newest revision too. Update the task "edited" badge to read from this.

- [x] **Task model**: Index `parent_id` (subtask lookups).

## Comments, files, wiki

- [x] **Comment model**: No `CommentRevision` table — comment edits are stored as `Revision` rows (`target_type = comment`) with an `AuditLog` entry pointing to them; deletions are soft deletes recorded in the `AuditLog`. Remove `edited_at: Option<i64>` — "last edited at" is the newest revision of the comment.

- [x] **FileRef model**: Generalize `FileRef` so files can attach to tasks *and* wiki pages: replace `task_id` with `target_type: String` (`task` or `page`) and `target_id: uuid::Uuid` (same shape as `ContentLink`). Composite key `(file_id, target_type, target_id)` — attached at most once per target; indexes on `file_id` and `target_id`. Update attachment and cleanup-unreferenced logic for both target types.

- [x] **Page content as a JSON field**: Drop the `Block` table. Store a page's content in `WikiPage.content: String` as versioned JSON, `{"version": 1, "blocks": [...]}`, using the existing validated block format (`backend/src/blocks.rs`; editor-independent, so a different editor never changes stored pages). A save is one row update, and "did anything change" is a comparison of the serialized JSON. `Revision` rows for pages store the same JSON. Search, references, export and live editing keep deriving from the block list as now. Markdown stays the export (and later import) format, not the storage format. When `version` changes, upgrade old content on read.

- [x] **Page model**: Rename `Page` struct to `WikiPage` in `backend/src/models.rs` (and update all references, including `PageRevision`, `PageDoc`, `ContentLink.source_page_id` naming if desired). This renames the table, so update the raw SQL in `backend/src/search.rs` (the triggers say `ON pages` and the search query joins `pages`); the OpenAPI schema names change too, so regenerate the client and update the frontend types.

## Operations

- [x] **Shutdown** (new): Handle SIGTERM as well as Ctrl+C (Docker stops containers with SIGTERM), and on shutdown save every open live-editing room before exiting, so the last seconds of typing are not lost.

- [x] **Background jobs** (new): A small scheduler (tokio intervals) for recurring work: deleting expired sessions, and later email digests. Runs in the server process; jobs must be safe to run twice.

- [x] **Backups** (new): A command (and admin action) that writes a consistent copy of the database with `VACUUM INTO`, plus a note in the README on backing up the data directory (database and files).

- [x] **Search** (new, small): The search query converts every page, task and comment id to hex on each search (full scans). Store `workspace_id` (and the project for tasks and comments) in `search_docs`, kept up to date by the triggers, so a search reads only matching rows. Fine as is for small installs; do it when renaming `Page` anyway.

## Verify last

- [x] **Integrity check** (replaces "verify foreign keys"): Toasty relations are enforced in its query engine, not as database constraints, so add an admin/CLI integrity check that reports orphaned rows (a task whose list or project is gone, a membership of a missing workspace or user, a `FileRef` to a missing file, a `ContentLink` to a missing source, ...). Skip polymorphic targets where a missing target is allowed (`ContentLink.target_id`, `AuditLog` ids). Run it as part of the tests against a seeded database.

- [x] **Migrations**: After all model changes above, replace `push_schema` with Toasty migrations, starting from this schema as the baseline.
