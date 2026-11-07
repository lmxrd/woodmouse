# AGENTS.md

Woodmouse is a self-hosted project manager: a Rust backend (axum, SeaORM on
SQLite) with a React frontend embedded in the one binary. README.md is the user
documentation and CONTRIBUTING.md the human contributor guide (task list, code
layout, conventions); read both for the full picture. This file is what an
agent must know before editing.

## Commands

- `mise install` — install the pinned toolchain
- `mise run dev` — both dev servers (`dev:backend` and `dev:frontend`)
- `mise run dev:backend` — API server (http://localhost:8000); hot reload
  via watchexec (restarts when `backend/src` or Cargo files change)
- `mise run dev:frontend` — frontend dev server
- `mise run migrate` — apply the pending migrations to the dev database (the backend also does it at start)
- `mise run seed` — fill an empty dev database with mock data (people, projects, tasks, wiki);
  sign in as `admin@woodmouse.test`, `sara@woodmouse.test` or `ola@woodmouse.test` (the `seed` binary, `backend/src/bin/seed.rs`; it is not part of the release build)
- `mise run check` — each stack's checks (clippy/rustfmt in `backend/`,
  typecheck/lint/format in `frontend/`)
- `mise run test` — each stack's tests: the backend's unit and integration tests (cargo test; integration
  through axum-test) and the frontend's unit tests (vitest)
- `mise run test:e2e` — the Playwright smoke tests (`packages/e2e`) against the image: builds it, starts it with
  a fresh database file in a volume (`scripts/e2e.sh`; `ENGINE`, `IMAGE`, `E2E_PORT`), installs Chromium only;
  CI runs it after the image build
- `mise run gen:spec` / `gen:sdk` — the OpenAPI spec, then the frontend's SDK generated from it
- `mise run build` — the release binary for this machine (its OS and CPU), frontend embedded, in
  `dist/woodmouse`
- `mise run build:linux` — the static (musl) release binary for this machine's
  CPU architecture, frontend embedded, written to `dist/woodmouse-linux-<arch>`
  (`scripts/build-linux.sh`, `ARCH`/`PROFILE`/`FEATURES` to vary it; cargo
  zigbuild makes zig the C toolchain, so on a Mac it builds a Linux binary);
  CI and the release workflow run this same task
- `mise run build:docker` — package that binary into the container image
  (`Dockerfile`: Alpine plus a `COPY` of the binary; the image builds nothing)
- `mise run fix` — each stack's auto-fixes
- `mise run audit` — known vulnerabilities in `Cargo.lock` and `pnpm-lock.yaml` (osv-scanner); `mise run audit:image`
  — in an image's Alpine packages (Trivy; `IMAGE=`, by default the published one). A finding to accept goes in
  `osv-scanner.toml`, with its reason
- `mise run clean` — delete the local dev database and uploaded files; the
  schema is recreated from the migrations on next backend start (only needed
  for a database made before migrations existed, or to start over)
- After changing the schema: add the next migration to `packages/backend/migration/src/` (an entity in
  `packages/backend/entity/src/` is a row of its table: run `mise run gen:models` and review the diff), and never edit one that has shipped

Run `mise run check` and `mise run test` before you call a change done.

## Dependencies

- **Use the community's crates and packages.** Prefer a well-known, maintained crate or npm package over writing
  the same thing ourselves. Propose it first (name, exact version, why) and get approval before adding it. Write it
  ourselves only when no maintained one fits, and say why in a comment where it lives (for example the session
  store: the community SQL store needs sqlx 0.8, and two bundled SQLites do not link).
- **Versions live in one place each, all exact.** Node in the root `package.json` (`devEngines.runtime`), pnpm in
  its `packageManager`, Rust (channel, components, musl targets) in `rust-toolchain.toml`, every other tool in
  `mise.toml` `[tools]`. mise reads all of them (`idiomatic_version_file_enable_tools`), so `mise install` (and
  CI's mise-action) installs everything. Crates are exact versions too (`axum = "0.8.9"`), the ones in
  `Cargo.lock`; shared crate versions live in `packages/backend/Cargo.toml`'s `[workspace.dependencies]`. The
  image builds nothing, so the Dockerfile pins no Node or Rust; its base is an exact Alpine release
  (`alpine:3.x.y`, with `apk upgrade`).
- **New versions wait a day.** pnpm refuses versions less than a day old (`minimumReleaseAge`, 1 day, in
  `pnpm-workspace.yaml`): take the newest that is older. It must stay shorter than Dependabot's cooldown, or it
  would block Dependabot's updates.
- **Dependency updates and security fixes** (`.github/dependabot.yml`): Dependabot's pull requests are `fix(deps)`
  for what ships (crates, the frontend's and SDK's dependencies, the base image), so merging one releases a patch,
  and `chore(deps-dev)` for devDependencies; `ci` for actions. Routine updates wait 5 days after their release
  (`cooldown`), security updates do not. Minor, patch and security updates merge on their own once CI passes
  (`dependabot-auto-merge.yml`); majors wait for a person. That merge is made with a workflow token and so starts
  no workflow: `release.yml` also runs daily to pick it up. `security.yml` runs `mise run audit` and scans the
  published image every day; a finding to accept goes in `osv-scanner.toml`, with its reason.

## Structure

- One git repository at this root; `packages/backend/` and `packages/frontend/` are stacks with their own
  AGENTS.md (follow those inside each directory). `packages/sdk/` is the frontend's API client, generated from the
  backend's OpenAPI spec (`mise run gen:sdk`), never committed; `packages/e2e/` the Playwright smoke tests.
- One pnpm workspace and lockfile at the root; `scripts/` for the Linux build and the end-to-end run.
- CI and releases: `.github/workflows/` (`ci.yml` runs `mise run check`/`test`, builds the image and scans it; pull
  request titles must be Conventional Commits (`pr-title.yml`), `release.yml` is release-please, `security.yml` runs
  `mise run audit` and scans the published image every day).

## Conventions

- **Keep it simple.** One binary, one SQLite file, no external services. Comment the *why*, not the what.
- **Generated, do not edit by hand:** `CHANGELOG.md` and the version fields (release-please),
  `packages/frontend/src/routeTree.gen.ts` (TanStack Router, committed), `pnpm-lock.yaml` and
  `packages/backend/Cargo.lock` (use the package managers), and `packages/sdk/src` (`mise run gen:sdk`, not
  committed).
- **Soft deletes only.** Projects, lists, tasks, comments, wiki pages and the like are moved to the trash, never
  destroyed, and every read must hide the deleted ones.
- **Frontend:** the URL holds the view, and its other rules are in `packages/frontend/AGENTS.md`.
- **Backend:** its structure and its conventions, in general and by feature, are in `packages/backend/AGENTS.md`.
- **Tests:** unit tests next to the code, integration tests of the API on axum-test
  (`packages/backend/api/tests/it/`), vitest for the frontend's `src/lib`, and a few end-to-end smoke tests
  (`packages/e2e`) that a real browser runs against the built image.

## Git and releases

Commit messages are **Conventional Commits** (`feat:`, `fix:`, `perf:`, `ci:`, `docs:`, ...): release-please reads
them to bump the version and write the changelog. So are **pull request titles**: pull requests are squash merged,
the commit on main takes the title, and release-please skips one that is not conventional. GitHub fills a new pull
request's title from the branch name: rewrite it. The `PR title` workflow fails a title that does not parse. No
`Co-Authored-By` trailer or "Generated with" line in commits or pull requests. Do not commit unless asked, and work
on a branch, not `main`. Releases are automated (see CONTRIBUTING.md); never tag by hand.
