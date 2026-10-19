# woodmouse

API service built with axum, utoipa (OpenAPI), and toasty on SQLite.

## Commands

| Command                | What it does                                        |
| ---------------------- | --------------------------------------------------- |
| `cargo run`           | Serve on http://localhost:8000 (Swagger at `/swagger-ui`) |
| `cargo test`          | Run tests (smoke test in `tests/api.rs`)           |
| `mise run check`      | rustfmt --check + clippy                            |
| `mise run fix`        | rustfmt + clippy --fix                              |
| `cargo run --bin specs` | Write `target/specs/openapi.{json,yaml}`          |

## Configuration

Environment variables (see `src/main.rs`):

- `BIND_ADDR` — listen address (default `0.0.0.0:8000`)
- `DATABASE_URL` — toasty connection URL (default `sqlite:woodmouse.db`;
  use `sqlite::memory:` for a throwaway database)

## Docker

```sh
docker build -t woodmouse .
docker run -p 8000:8000 -v woodmouse-data:/data woodmouse
```
