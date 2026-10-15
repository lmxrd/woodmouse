# woodmouse

Monorepo: mise-managed toolchain at the root, stacks in `backend/` and
`frontend/` (each with its own AGENTS.md).

The `frontend/` is a CSR React SPA (vite) whose build output (`frontend/dist`)
is embedded into the Rust backend binary at compile time (rust-embed). The
backend serves it at `/` with an index.html fallback for SPA routes; no static
file server is needed in production.

## What it does

Woodmouse is a self-hosted task manager for a household or small team.

- **Workspaces** hold projects and members. Roles are admin, write and read.
- **Projects** contain lists. Each project has a **board** (one column per list plus a Done column) and a
  **list view** with filters and sorting; the view, filters and sort live in the URL, so they can be bookmarked.
- **Tasks** have a title, description, priority, start and due dates, assignees, labels, and a completed
  state. Completing a task is a state, not a list: completed tasks appear in the board's Done column
  wherever they came from, and reopening one returns it to its list. Drag cards between columns, or use the
  checkbox.
- **My tasks** shows everything assigned to you across a workspace, grouped by due date.
- **Settings** (per workspace) manage members and labels.
- Works on phones: the sidebar becomes a drawer, and the task dialog goes full-screen.

### First run and members

1. Open the app on a fresh database: the first account you create is the system super admin.
2. Create a workspace, then add people under **Settings → Members** (admins only). A new person gets a
   **one-time invite link** that the admin copies from the page and shares; it works once and expires after
   seven days. Opening it lets them choose a password and signs them in. Adding someone who already has an
   account just makes them a member.
3. There is no email or self-service sign-up, so an admin can always issue a fresh link for a pending member.

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
| `mise run clean`         | Delete the local dev database (the schema is recreated on next start; there are no migrations yet) |

For development, run both dev servers: vite on :5173 with the API on :8000
(vite proxies `/api` to the backend, so cookies stay same-origin). `cargo build` in `backend/` requires `frontend/dist`
to exist — build the frontend first (or use `mise run build`).

Docker (from the repo root; the image generates the spec and client,
then builds the frontend and embeds it):

    docker build -f backend/Dockerfile -t woodmouse .
    docker run -p 8000:8000 -v woodmouse-data:/data woodmouse
