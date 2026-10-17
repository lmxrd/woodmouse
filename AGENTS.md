# AGENTS.md

## Commands

- `mise install` — install the pinned toolchain
- `mise run dev:backend` — API server (http://localhost:8000); hot reload
  via watchexec (restarts when `backend/src` or Cargo files change)
- `mise run dev:frontend` — frontend dev server
- `mise run seed` — fill an empty dev database with mock data (people, projects, tasks, wiki);
  sign in as `admin@woodmouse.test`, `sara@woodmouse.test` or `ola@woodmouse.test` (see `backend/src/seed.rs`)
- `mise run check` — each stack's checks (clippy/rustfmt in `backend/`,
  typecheck/lint/format in `frontend/`)
- `mise run test` — each stack's tests (cargo test, vitest)
- `mise run build:docker` — build the container image (`Dockerfile`)
- `mise run fix` — each stack's auto-fixes
- `mise run clean` — delete the local dev database and uploaded files; the
  schema is recreated from the migrations on next backend start (only needed
  for a database made before migrations existed, or to start over)
- After changing `backend/src/models.rs`: `cargo run --bin migrate -- generate <name>`
  in `backend/`, and commit `backend/toasty/`

## Structure

- CI and releases: `.github/workflows/` (`ci.yml` runs `mise run check`/`test` and builds the image; pull request
  titles must be Conventional Commits, `release.yml` is release-please).
- One git repository at this root; `backend/` and `frontend/` are scaffolded
  stacks with their own AGENTS.md (follow those inside each directory).
- Tool versions are pinned in `mise.toml`; keep the file pruned to the tools
  actually used by the scaffolded stacks.
