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

## Dependencies

- **Dependencies.** Prefer a well-known, maintained crate or package over writing the same thing ourselves.
  Propose it first (name, exact version, why) and get approval before adding it. Write it ourselves only when no
  maintained one fits, and say why in a comment where it lives (for example the session store: the community
  SQL store needs sqlx 0.8, and two bundled SQLites do not link).
  Shared crate versions live in `packages/backend/Cargo.toml`'s `[workspace.dependencies]`.

## Structure

- CI and releases: `.github/workflows/` (`ci.yml` runs `mise run check`/`test`, builds the image and scans it; pull
  request titles must be Conventional Commits, `release.yml` is release-please, `security.yml` runs `mise run audit` and
  scans the published image every day).
- Dependency updates and security fixes (`.github/dependabot.yml`): Dependabot's pull requests are `fix(deps)` for what
  ships (crates, the frontend's and SDK's dependencies, the base image), so merging one releases a patch, and
  `chore(deps-dev)` for devDependencies; `ci` for actions. Routine updates wait 5 days after their release
  (`cooldown`), security updates do not. Minor, patch and security updates merge on their own once CI passes
  (`dependabot-auto-merge.yml`); majors wait for a person. That merge is made with a workflow token and so starts no
  workflow: `release.yml` also runs daily to pick it up. pnpm's `minimumReleaseAge` (1 day, `pnpm-workspace.yaml`)
  must stay shorter than the cooldown, or it would block Dependabot's updates. The image's base is an exact Alpine
  release (`alpine:3.x.y`, with `apk upgrade`).
- One git repository at this root; `backend/` and `frontend/` are scaffolded
  stacks with their own AGENTS.md (follow those inside each directory).
- Tool versions, each in one place, all exact: Node in the root `package.json` (`devEngines.runtime`), pnpm in
  its `packageManager`, Rust (channel, components, musl targets) in `rust-toolchain.toml`, every other tool in
  `mise.toml` `[tools]`. mise reads all of them (`idiomatic_version_file_enable_tools`), so `mise install` (and
  CI's mise-action) installs everything. Crates are exact versions too (`axum = "0.8.9"`), the ones in
  `Cargo.lock`. The image builds nothing, so the Dockerfile pins no Node or Rust.
