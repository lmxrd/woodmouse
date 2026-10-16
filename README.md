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
- **Projects** contain lists, and start with an **Inbox** list for unorganized tasks (it can't be deleted; move
  tasks out of it when you've sorted them). A project has four views: a **board** (one column per list plus a
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
  a page; external links can use any URL format. Edits save automatically. If someone else is editing the
  same page you are told, and a save based on an outdated version is refused so nothing is silently
  overwritten (keep yours or load theirs). Deleted pages go to the trash with their subpages.
- **My tasks** shows everything assigned to you across a workspace, grouped by due date.
- **Trash**: nothing is permanently deleted. Deleting a project, list, task, comment, label or view moves it
  (with what it contains) to the trash, where members with write access can restore it. The only permanent
  deletion is the admin's unused-files cleanup.
- **Settings** (per workspace) manage members and labels. **Account** (per user) edits your display name,
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
| `MAX_UPLOAD_MB`    | `25`                   | Largest accepted upload                                              |

Files are stored behind a small `FileStore` trait (`backend/src/storage.rs`) and each file records which backend
holds it, so another store (for example a Nextcloud/WebDAV share) can be added later, and several can coexist.

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
| `mise run clean`         | Delete the local dev database and uploaded files (the schema is recreated on next start; there are no migrations yet) |

For development, run both dev servers: vite on :5173 with the API on :8000
(vite proxies `/api` to the backend, so cookies stay same-origin). `cargo build` in `backend/` requires `frontend/dist`
to exist — build the frontend first (or use `mise run build`).

Docker (from the repo root; the image generates the spec and client,
then builds the frontend and embeds it):

    docker build -f backend/Dockerfile -t woodmouse .
    docker run -p 8000:8000 -v woodmouse-data:/data woodmouse

The `/data` volume holds both the database and the uploaded files.
