# AGENTS.md

## Commands

- `mise install` — install the pinned toolchain
- `mise run dev:backend` — API server (http://localhost:8000); hot reload
  via watchexec (restarts when `backend/src` or Cargo files change)
- `mise run dev:frontend` — frontend dev server
- `mise run migrate` — apply the pending migrations to the dev database (the backend also does it at start)
- `mise run seed` — fill an empty dev database with mock data (people, projects, tasks, wiki);
  sign in as `admin@woodmouse.test`, `sara@woodmouse.test` or `ola@woodmouse.test` (the `seed` binary, `backend/src/bin/seed.rs`; it is not part of the release build)
- `mise run check` — each stack's checks (clippy/rustfmt in `backend/`,
  typecheck/lint/format in `frontend/`)
- `mise run test` — each stack's tests (cargo test, vitest)
- `mise run build:backend` — the static (musl) release binary for this machine's
  CPU architecture, frontend embedded (zig is the C toolchain, so on a Mac it
  builds a Linux binary); CI and the release workflow run this same task
- `mise run build:docker` — package that binary into the container image
  (`Dockerfile`: distroless, nothing compiles in it)
- `mise run fix` — each stack's auto-fixes
- `mise run clean` — delete the local dev database and uploaded files; the
  schema is recreated from the migrations on next backend start (only needed
  for a database made before migrations existed, or to start over)
- After changing the schema: add the next migration to `packages/backend/migration/src/` (an entity in
  `packages/backend/entity/src/` is a row of its table: run `mise run gen:models` and review the diff), and never edit one that has shipped

## Structure

- CI and releases: `.github/workflows/` (`ci.yml` runs `mise run check`/`test` and builds the image; pull request
  titles must be Conventional Commits, `release.yml` is release-please).
- One git repository at this root; `backend/` and `frontend/` are scaffolded
  stacks with their own AGENTS.md (follow those inside each directory).
- Tool versions are pinned in `mise.toml`; keep the file pruned to the tools
  actually used by the scaffolded stacks.
