# AGENTS.md

## Commands

- `pnpm dev` — dev server at http://localhost:5173
- `pnpm build` — typecheck (`tsc -b`) then production build
- `pnpm test` — run vitest tests
- `pnpm check` — typecheck + oxlint + oxfmt --check (read-only; run before
  committing)
- `pnpm fix` — auto-fix lint issues and reformat (oxlint --fix + oxfmt)
- `pnpm preview` — serve the production build

- Dependencies: before adding an npm package, check whether the functionality
  is covered by the Deno Standard Library (`@std/*`,
  https://github.com/denoland/std) or its extension stdx (`@stdx/*`,
  https://github.com/lmxrd/std-js). If it is, prefer the JSR package:
  `pnpm dlx jsr add @std/<pkg>` — this writes an alias to `package.json` and
  a registry entry to `.npmrc`; commit both.

## Dependencies

- Before adding an npm package, check whether the functionality is covered
  by the Deno Standard Library (`@std/*`, https://github.com/denoland/std) or
  its extension stdx (`@stdx/*`, https://github.com/lmxrd/std-js). If it is,
  prefer the JSR package: `pnpm add jsr:@std/<pkg>` (pnpm >= 12 supports the
  `jsr:` specifier natively; imports stay `@std/<pkg>`).

## Conventions

- Routing: file-based TanStack Router. Each route is a file in `src/routes/`
  using `createFileRoute` (see the existing files). The plugin in
  `vite.config.ts` regenerates `src/routeTree.gen.ts` on dev/build — it is
  committed; do not edit it by hand.
- Shared UI goes in `src/components/` (e.g. `RootLayout`); shared helpers and
  clients go in `src/lib/` (e.g. `queryClient`).
- Data fetching: TanStack Query (`useQuery`/`useMutation`). Tune defaults in
  `src/lib/queryClient.ts`; wire real endpoints by replacing `queryFn`s in
  route components.
- Styling: Tailwind CSS v4 utility classes. Global styles live only in
  `src/index.css`; do not add a `tailwind.config.js`.
- TypeScript strict; unused imports/locals are build errors — clean up as
  you go.
- Lint/format: oxlint (`.oxlintrc.json`) and oxfmt (`.oxfmtrc.json`, which
  ignores the generated route tree); prefer `pnpm fix` over hand-formatting.
- Tests: vitest for the utils in `src/lib` only — pure functions, node
  environment, colocated `*.test.ts` (see `src/lib/utils.test.ts`). Do not
  add component/visual tests.

## Imports

- Use relative imports; there is no `@/` alias in the app or the bundler.
- `tsconfig.json` keeps a `paths` entry for `@/*` only so the shadcn CLI can
  resolve its targets. After `pnpm dlx shadcn@latest add <name>`, rewrite the
  generated `@/lib/utils` and `@/components/ui/*` imports to relative paths
  (`../../lib/utils`, `./button`); `pnpm check` fails if one is left behind.
