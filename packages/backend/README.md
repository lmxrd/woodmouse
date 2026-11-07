# woodmouse (backend)

The server: axum, SeaORM on SQLite, the OpenAPI spec (utoipa), and the
frontend embedded in the one binary. The user documentation is the
[root README](../../README.md); the rules for changing this code are in
[AGENTS.md](AGENTS.md).

| Command (from the repository root) | What it does |
| --- | --- |
| `mise run dev:backend` | Serve on http://localhost:8000, restarted on changes |
| `mise run test:backend` | Unit tests and the API's integration tests (axum-test) |
| `mise run check:backend` | rustfmt and clippy (`-D warnings`) |
| `mise run gen:spec` | Write the OpenAPI spec to `target/specs/` (JSON and YAML) |
| `mise run seed` | Mock data in an empty dev database |

A Cargo workspace: `src/` is the server binary (with the embedded frontend), and `api/` the library with
everything else. In `api/`, `src/` holds the API (`routes/`) and `tests/it/` the integration tests (one file
per area); `entity/` holds the database models and `migration/` the schema (a new migration per change).
