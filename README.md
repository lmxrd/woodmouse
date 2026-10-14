# woodmouse

Monorepo: mise-managed toolchain at the root, stacks in `backend/` and
`frontend/` (each with its own AGENTS.md).

The `frontend/` is a CSR React SPA (vite) whose build output (`frontend/dist`)
is embedded into the Rust backend binary at compile time (rust-embed). The
backend serves it at `/` with an index.html fallback for SPA routes; no static
file server is needed in production.

## Commands

| Command                  | What it does                                        |
| ------------------------ | --------------------------------------------------- |
| `mise install`           | Install the pinned tools (mise.toml)                |
| `mise run dev:backend`   | API server (http://localhost:8000)                  |
| `mise run dev:frontend`  | Frontend dev server (http://localhost:5173)         |
| `mise run build`         | Generate client from spec, build frontend, then backend with the bundle embedded|
| `mise run check`         | Each stack's checks (clippy/fmt, lint/types)       |
| `mise run fix`           | Each stack's auto-fixes                             |

For development, run both dev servers: vite on :5173 with the API on :8000
(vite proxies `/api` to the backend, so cookies stay same-origin). `cargo build` in `backend/` requires `frontend/dist`
to exist — build the frontend first (or use `mise run build`).

Docker (from the repo root; the image generates the spec and client,
then builds the frontend and embeds it):

    docker build -f backend/Dockerfile -t woodmouse .
    docker run -p 8000:8000 -v woodmouse-data:/data woodmouse
