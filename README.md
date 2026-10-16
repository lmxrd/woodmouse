# woodmouse

Monorepo: mise-managed toolchain at the root, stacks in `backend/` and
`frontend/` (each with its own AGENTS.md).

The `frontend/` is a CSR React SPA (vite) whose build output (`frontend/dist`)
is embedded into the Rust backend binary at compile time (rust-embed). The
backend serves it at `/` with an index.html fallback for SPA routes; no static
file server is needed in production.

## What it does

Woodmouse is a self-hosted task manager for a household or small team.

- **Workspaces** hold projects, files and members. Roles are admin, write and read.
- **Projects** contain lists. A task in no list is in the project's **Inbox** (the first board column; quick adds
  go there, and you move tasks out of it when you've sorted them). A project has four views: a **board** (one column per list plus a
  Done column), a **list view** with filters and sorting, a **calendar** (month or week; drag tasks to
  reschedule, quick-add on a day, an unscheduled panel, an agenda on phones) and a **timeline** (Gantt: bars
  from start to due date, subtasks nested, parents derived from their subtasks, dependency arrows from
  "blocks" links with violations marked, drag to move or resize). Everything about what you see lives in the
  URL, so a copied link shows the same thing: the view, the assignee, label, list and status filters (shared by
  every view), the sort, the calendar's day and zoom, and the timeline's zoom and position (the date at the
  centre of the screen). The Files page keeps its search there too. A view can be saved as a named **view**,
  private to you or shared with the workspace.
- **Tasks** have a markdown description, priority, start and due dates, assignees, labels and a completed
  state. Completing a task is a state, not a list: completed tasks appear in the board's Done column wherever
  they came from, and reopening one returns it to its list. Drag cards between columns and within a column
  (mouse, touch or keyboard), or use the checkbox.
- **Subtasks** nest to any depth, with progress rolled up to every parent. Tasks can be **linked** (blocks,
  relates to, duplicates).
- **Comments** (markdown), **attachments** and a per-task and per-project **activity feed**. A workspace-wide
  feed shows everything.
- **Files**: every upload goes into the workspace's library, deduplicated by content. Attaching a file to a
  task does not own it: deleting the task keeps the file, and only an admin's "clean up unused files" removes
  files nothing references.
- **Wiki**: a tree of pages per workspace (the sidebar shows three levels, the Wiki overview all of them).
  Pages are built from blocks (text, headings, bulleted, numbered and to-do lists, quotes, callouts, code,
  dividers) with markdown shortcuts, a `/` menu, a floating toolbar, `@` to mention a task and `[[` to link
  a page; external links can use any URL format. Pages are **edited live** with everyone else who has them
  open: changes and cursors show up as people type, and the server saves the page a couple of seconds after
  the last edit (history, links, search and export all keep working from the saved page). If the live
  connection cannot be made, the page falls back to **manual mode**: autosave is off, a Save button appears,
  a stale save is refused with a keep-mine/load-theirs choice, and everyone else with the page open is
  warned that someone is editing without live sync (saving replaces the live version). Deleted pages go to the trash with their subpages.
- **Links between notes and tasks**: a page's `@task` mentions and `[[page]]` links are recorded on every
  save, so a page shows "Referenced by" the pages that link to it and a task shows "Mentioned in". Links
  hold ids, and show the target's current title (the stored label is only a fallback); a link to something
  in the trash is struck through and works again when it is restored. Pages can be dragged in the sidebar
  and the wiki overview: onto the middle of a page to nest, onto an edge to place beside it.
- **History and export**: a page keeps earlier versions (a version is saved when someone else edits, after a
  ten-minute pause, or once an hour in a long session). The page menu's Version history shows them with a
  preview and restores one; the content being replaced is kept first, so a restore can be undone. A task's
  title and description have the same history. Pages export as Markdown (one page from its menu, the whole
  wiki as a zip from the Wiki overview, with folders following the tree and links between pages kept).
- **Calendar links**: subscribe from any calendar app (Apple, Google, Thunderbird, ...) to a read-only feed of
  tasks with dates: your own tasks, a project, or a whole workspace. Ranges span their days, done tasks are
  marked, undated tasks are left out. Links are secret URLs (anyone who has one sees those tasks); you can
  name them, see when they were last used, and revoke one or all from your account page, a workspace's
  Calendar settings tab, or a project's menu. A link stops working while its owner cannot see what it shows.
  Set `PUBLIC_URL` if the server is reached through a different address than the one in the browser (the links
  inside events use it).
- **Themes**: four colour palettes (Paper and pine, Slate, Mono, Lavender), each in light and dark, plus any accent
  colour. Admins set a workspace's palette and accent (workspace settings, Appearance tab); each member can use
  their own for a workspace from their account page, where light, dark or follow-the-device is also chosen
  once for the whole account.
- **Search**: ⌘K (or Ctrl+K) searches page and task titles and text with SQLite full-text search (words match
  as prefixes, titles count more than text) in this workspace or in all workspaces you belong to. Soft-deleted
  items are never found. Task comments are searched too (a match shows as its task). The `@` and `[[` pickers use the same search.
- **My tasks** shows everything assigned to you across a workspace, grouped by due date.
- **Trash**: nothing is permanently deleted. Deleting a project, list, task, comment, label, view or page moves it
  (with what it contains) to the trash, where members with write access can restore it. A deleted workspace
  disappears for everyone and only the site's super admin can bring it back. The only permanent deletions are
  expired login sessions and the admin's unused-files cleanup.
- **History and audit**: every save of a task or comment, and page saves grouped by editing session, keep an
  earlier version you can view and restore. The super admin can read the site-wide **audit log** (sign-ins,
  role changes, deletions, edits; it records who and when, never an IP address) and remove the content of a
  version where something sensitive was saved. Disabling an account blocks it but keeps everything it made.
- **Settings** (per workspace) manage the name and description, members and labels, and (for admins) deleting the
  workspace. **Account** (per user) edits your display name,
  email and password, and how dates and times are shown (date format, 12 or 24 hour clock, and the first day
  of the week). Items from the
  last seven days show relative times ("2 h ago"); older ones show the date and time in your format.
- Works on phones: the sidebar becomes a drawer, dialogs go full-screen, and cards drag by long press.

### First run and members

1. Open the app on a fresh database: the first account you create is the system super admin.
2. Create a workspace, then add people under **Settings → Members** (admins only). A new person gets a
   **one-time invite link** that the admin copies from the page and shares; it works once and expires after
   seven days. Opening it lets them choose a password and signs them in. Adding someone who already has an
   account just makes them a member.
3. There is no email or self-service sign-up, so an admin can always issue a fresh link for a pending member.

### Configuration

The backend reads these environment variables (all optional):

| Variable           | Default                | Meaning                                                              |
| ------------------ | ---------------------- | -------------------------------------------------------------------- |
| `PORT`             | `8000`                 | HTTP port                                                            |
| `DATABASE_URL`     | `sqlite:woodmouse.db`  | The SQLite database                                                  |
| `DATA_DIR`         | `data`                 | Where uploaded files live (`<DATA_DIR>/files`) with the local store  |
| `STORAGE_BACKEND`  | `local`                | Where new uploads are stored; `local` is the only backend for now    |
| `PUBLIC_URL`       | from the request       | Address of the app for links inside calendar events (e.g. `https://tasks.example.com`) |
| `MAX_UPLOAD_MB`    | `25`                   | Largest accepted upload                                              |
| `BACKUP_DIR`       | `backups` (manual only)| Where backups go; when set the server also writes one a day and keeps the newest 14 |

Files are stored behind a small `FileStore` trait (`backend/src/storage.rs`) and each file records which backend
holds it, so another store (for example a Nextcloud/WebDAV share) can be added later, and several can coexist.

### Backups and checks

- `woodmouse backup [DIR]` (or the super admin's `POST /api/admin/backup`) writes a consistent copy of the
  database with SQLite's `VACUUM INTO`, safe while the server runs. Uploaded files are not part of it: back up
  `DATA_DIR` too (database and files together are the whole state).
- `woodmouse check` (or `GET /api/admin/integrity`) reports rows that point at something that no longer exists.
- The schema evolves through migrations in `backend/toasty/`, applied at startup. On SIGTERM the server saves
  any pages being edited live before it exits.

### Behind a reverse proxy

Live editing uses a WebSocket (`/api/pages/<id>/live`), so a proxy in front of the app must pass WebSocket
upgrades through (nginx: `proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection "upgrade";`;
Caddy and Traefik do this by default). The app keeps live documents in memory, so run one instance. Without
a working WebSocket, pages still work in manual mode.

### API

All routes live under `/api` and are documented in the OpenAPI spec (Swagger UI at
`http://localhost:8000/swagger-ui`). The frontend client in `frontend/packages/client` is generated from the
spec by `mise run gen:client`.

## Commands

| Command                  | What it does                                        |
| ------------------------ | --------------------------------------------------- |
| `mise install`           | Install the pinned tools (mise.toml)                |
| `mise run dev:backend`   | API server (http://localhost:8000), restarts on code changes |
| `mise run dev:frontend`  | Frontend dev server (http://localhost:5173)         |
| `mise run build`         | Generate client from spec, build frontend, then backend with the bundle embedded|
| `mise run check`         | Each stack's checks (clippy/fmt, lint/types)       |
| `mise run fix`           | Each stack's auto-fixes                             |
| `mise run clean`         | Delete the local dev database and uploaded files (the schema is recreated from the migrations on next start) |

For development, run both dev servers: vite on :5173 with the API on :8000
(vite proxies `/api` to the backend, so cookies stay same-origin). `cargo build` in `backend/` requires `frontend/dist`
to exist — build the frontend first (or use `mise run build`).

Docker (from the repo root; the image generates the spec and client,
then builds the frontend and embeds it):

    docker build -f backend/Dockerfile -t woodmouse .
    docker run -p 8000:8000 -v woodmouse-data:/data woodmouse

The `/data` volume holds both the database and the uploaded files.
