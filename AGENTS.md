# AGENTS.md

## Commands

- `mise install` — install the pinned toolchain
- `mise run dev:backend` — API server (http://localhost:8000); hot reload
  via watchexec (restarts when `backend/src` or Cargo files change)
- `mise run dev:frontend` — frontend dev server
- `mise run check` — each stack's checks (clippy/rustfmt in `backend/`,
  typecheck/lint/format in `frontend/`)
- `mise run fix` — each stack's auto-fixes
- `mise run clean` — delete the local dev database and uploaded files; the
  schema is recreated on next backend start (use after schema changes; no
  migrations yet)

## Structure

- One git repository at this root; `backend/` and `frontend/` are scaffolded
  stacks with their own AGENTS.md (follow those inside each directory).
- Tool versions are pinned in `mise.toml`; keep the file pruned to the tools
  actually used by the scaffolded stacks.
