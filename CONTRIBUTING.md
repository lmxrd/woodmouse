# Contributing to Woodmouse

Woodmouse is a Rust backend with a React frontend, built to be simple to run:
one binary, one SQLite file, no external services. Changes that keep it that
way are the changes we want.

## Getting set up

Install [mise](https://mise.jdx.dev) and run `mise install`. It brings the pinned
Rust toolchain, Node, pnpm and zig. Then:

```bash
mise run dev      # the backend on :8000 and the Vite dev server on :5173, together
mise run seed     # mock people, projects, tasks and wiki pages, into an empty dev database
```

Open the Vite URL, <http://localhost:5173>. Vite proxies `/api` to the
backend, so cookies stay on one origin. Sign in as `admin@woodmouse.test`,
`sara@woodmouse.test` or `ola@woodmouse.test`, all with the password
`password`. To start over, run `mise run clean` and seed again.

## Tasks

| Task                    | What it does                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------- |
| `mise run dev`          | Runs the backend (restarts on changes) and the frontend dev server together                       |
| `mise run dev:backend`  | The backend only, on :8000                                                                        |
| `mise run dev:frontend` | The frontend dev server only                                                                      |
| `mise run db:migrate`   | Applies the pending migrations to the dev database (the backend also does this when it starts)    |
| `mise run gen:models`   | Regenerates the entities in `packages/backend/src/models` from the migrated schema (review the diff) |
| `mise run seed`         | Fills an empty dev database with mock data                                                        |
| `mise run clean`        | Deletes the local dev database and uploaded files; the schema is recreated on the next start      |
| `mise run check`        | Formatting, lint and type checks for both stacks (rustfmt, clippy, tsc, oxlint, oxfmt)            |
| `mise run fix`          | Applies the format and lint fixes                                                                 |
| `mise run test`         | The backend tests (integration tests over the API, and unit tests) and the frontend tests (vitest)  |
| `mise run gen:spec`     | Writes the OpenAPI spec to `packages/backend/target/specs/`                                       |
| `mise run gen:sdk`      | Generates the frontend's SDK in `packages/sdk` from the spec (not committed)                      |
| `mise run build`        | Builds the frontend and the static release binary for your machine's CPU architecture             |
| `mise run build:docker` | Builds the container image from that binary (podman), for your machine's architecture             |

`mise run check` and `mise run test` must pass before a change can land. They
are also what CI runs.

## The repository

The JavaScript side is a pnpm workspace with one lockfile at the root.

```
packages/backend/    the Rust server: axum, utoipa (OpenAPI) and SeaORM on SQLite
packages/frontend/   the React app (Vite, TanStack Router and Query, Tailwind)
packages/sdk/        the frontend's API client, generated from the OpenAPI spec; never edited
.github/             CI, the release workflow and Dependabot
mise.toml            the toolchain and every task
```

Inside the backend:

```
src/main.rs        entrypoint: startup, the commands (check, healthcheck), graceful stop
src/bin/           tools that are not part of the release build: `migrate` (apply the migrations), `seed` (mock data), `specs` (the OpenAPI spec)
src/lib.rs         the library crate the server and the tools share
src/routes/        the HTTP handlers, one module per area
src/models/       the database models: one generated SeaORM entity per table (`mise run gen:models`)
src/content/       tasks, wiki pages, revisions, trash, ordering, search
src/live/          live wiki editing (Yjs rooms over WebSockets)
src/transfer/      import and export
src/jobs.rs        the daily and periodic jobs
src/db.rs          the pool, `Db` and the `prelude` of SeaORM traits
src/migrations/   the schema: SeaORM migrations, each running a SQL file
tests/it/          the end-to-end tests
```

Each stack has its own `AGENTS.md` with the detailed conventions: read
[the backend's](packages/backend/AGENTS.md) and
[the frontend's](packages/frontend/AGENTS.md) before a larger change.

## The API and its spec

Every endpoint is documented in code: a `#[utoipa::path]` on the handler,
named request and response types with `ToSchema`, and the handler registered
with `routes!` in `src/routes/mod.rs`, which is both the route and the spec
entry, so the two cannot drift. `mise run gen:spec` writes the spec and
`mise run gen:sdk` generates the frontend's SDK from it. The generated SDK is
not committed, and the checks and tests generate it when they need it.

Handler names become operation ids in the SDK, so they must be unique across
modules.

## Changing the schema

The schema is a list of SeaORM migrations in `packages/backend/src/migrations/`. Add a
`mYYYYMMDD_HHMMSS_what_it_does.rs` (and its `.sql` file, which it runs), register it in
`src/migrations/mod.rs`, run `mise run gen:models` to update the entities in `src/models/`. The server
applies pending migrations, in order, when it starts, and records each in
`seaql_migrations`. The tables are plain SQL because the schema needs what the schema
builder does not express (CHECK constraints, foreign key actions, FTS5, table rebuilds).

Never edit a migration that has shipped. Full-text search (FTS5 table and triggers) lives
in the migrations too; a migration that rebuilds `tasks`, `comments` or `wiki_pages` must
create their search triggers again, because SQLite drops a table's triggers with it.

## Conventions

- **Soft deletes only.** Projects, lists, tasks, comments, wiki pages and the
  like are moved to the trash, never destroyed, and every read must hide the
  deleted ones.
- **Keep it simple.** One binary, one SQLite file. If a change needs a new
  dependency or a new moving part, first check whether the plain version is
  enough.
- **The URL holds the view.** In the frontend, anything that changes what a
  list, board, calendar, timeline or page shows is a search parameter, so a
  copied link reproduces the view.
- **Comment the why, not the what.** The reasoning behind a decision is the
  part the code cannot show.

## Pull requests, commits and releases

Releases are automated with [release-please](https://github.com/googleapis/release-please),
driven by the commit messages on `main`. Write them as
[Conventional Commits](https://www.conventionalcommits.org): `feat:` bumps the
minor version, `fix:` the patch, and `feat!:` (or a `BREAKING CHANGE:` footer)
the major. `ci:`, `chore:`, `docs:` and the like release nothing.

Pull requests are squash merged with the pull request's title as the commit
message, so **the title must be a Conventional Commit** (`feat: add ...`,
`fix(auth): ...`), not the branch name GitHub fills in. A `PR title` check fails
the pull request otherwise, and release-please skips a commit that is not
conventional.

Every push to `main` updates one open release PR with the version bump in
`packages/backend/Cargo.toml` and `Cargo.lock`, `packages/frontend/package.json`
and `packages/sdk/package.json`, plus the changelog. Merging it tags the
release, and the release workflow:

- builds the static Linux binaries for `amd64` and `arm64` natively, on a
  runner of each architecture, and attaches them to the release with their
  checksums and the OpenAPI spec;
- packages those same binaries into the Docker image and pushes it to
  `ghcr.io/lmxrd/woodmouse` as one multi-arch image, tagged `1.2.3`, `1.2` and
  `latest`.

CI builds the same binary on every pull request and starts the image to check
that it is healthy, so a packaging mistake fails the pull request, not the
release.

### Building a release binary yourself

`mise run build` produces a static `musl` binary for your machine's CPU
architecture, with the frontend built in. zig is the C toolchain for the bundled
SQLite, so the same command works on Linux and on a Mac, where it produces a
Linux binary. The result is `packages/backend/target/dist/woodmouse`.
