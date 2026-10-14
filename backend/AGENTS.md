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
