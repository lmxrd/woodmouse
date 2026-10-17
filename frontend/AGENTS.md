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
- UI goes in `src/components/`, one folder per feature (`task/`, `board/`, `wiki/`, `admin/`, ...); `layout/` is the
  app shell (`RootLayout`, `Sidebar`), `common/` the small pieces used everywhere (dialogs, fields, avatars) and `ui/`
  the shadcn primitives. Shared helpers and
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

- **View state belongs in the URL.** Anything that changes what a list, board, calendar, timeline or page shows
  (filters, sort, dates, zoom, search) is a validated search param (`validateSearch`; project views share
  `lib/projectSearch.ts` and `ProjectFilters`), never local state, so a link always reproduces the view.
  Write explicit values (not "today" or a default that can change); update with `replace: true`.

- Wiki editor: `PageEditor` picks live editing (`LiveBody`: Yjs + y-websocket + TipTap collaboration; the
  server saves) or, if the socket cannot connect, `ManualBody` (no autosave, Save button, version check).
  Mentions and page links save only `id` and `label`; their current titles come from `useReferences` via the
  `References` extension storage, never from node attributes (attributes sync to everyone and are saved).

- Themes: colours live in `src/lib/themes.ts` (palettes in light and dark, turned into CSS at startup) and are
  read through the semantic tokens in `index.css`; never hardcode colours in components. `useTheme` (in
  `RootLayout`) applies the person's own choice for the workspace or else the workspace's, plus their light/dark
  mode; `--accent-color` (set from the chosen accent) overrides the palette's own accent. A new palette goes
  in the `PALETTES` list and the backend's `themes::PALETTES`.
