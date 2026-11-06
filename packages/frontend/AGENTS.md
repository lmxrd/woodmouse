# AGENTS.md

## Commands

- `pnpm dev` — dev server at http://localhost:5173
- `pnpm build` — typecheck (`tsc -b`) then production build
- `pnpm test` — run vitest tests
- `pnpm check` — typecheck + oxlint + oxfmt --check (read-only; run before
  committing)
- `pnpm fix` — auto-fix lint issues and reformat (oxlint --fix + oxfmt)
- `pnpm preview` — serve the production build

## Dependencies

- Prefer a well-known, maintained package over writing the same thing ourselves. Propose it first (name, exact
  version, why) and get approval before adding it, then add it at that exact version. Write it ourselves only when
  no maintained one fits, and say why in a comment where it lives.
- For general utilities, the Deno Standard Library (`@std/*`, https://github.com/denoland/std) and its extension
  stdx (`@stdx/*`, https://github.com/lmxrd/std-js) count as well-known: `pnpm add jsr:@std/<pkg>` (pnpm >= 12
  supports the `jsr:` specifier natively; imports stay `@std/<pkg>`).

## Conventions

- Routing: file-based TanStack Router. Each route is a file in `src/routes/`
  using `createFileRoute` (see the existing files). The plugin in
  `vite.config.ts` regenerates `src/routeTree.gen.ts` on dev/build — it is
  committed; do not edit it by hand.
- UI goes in `src/components/`, one folder per feature (`task/`, `board/`, `wiki/`, `admin/`, ...); `layout/` is the
  app shell (`RootLayout`, `Sidebar`), `common/` the small pieces used everywhere (dialogs, fields, avatars) and `ui/`
  the shadcn primitives. Shared helpers and
  clients go in `src/lib/` (e.g. `query-client`). File names under `src/components/` and `src/lib/` are
  kebab-case (`task-detail.tsx`, `use-theme.ts`, `page-tree.test.ts`); the exported component or hook keeps its
  own name (`TaskDetail`, `useTheme`). `src/routes/` keeps TanStack Router's file-route names.
- shadcn/ui runs on Base UI (`@base-ui/react`, style `base-vega` in `components.json`), not Radix. There is no
  `asChild`: pass the element to `render` (`<PopoverTrigger render={<Button variant="outline" />}>Edit</PopoverTrigger>`),
  and style a link as a button with `buttonVariants` on the `<a>`/`<Link>` itself. A `Select` shows the raw value
  unless the root gets `items` (a `{ value: label }` record or `{ value, label }[]`); its `onValueChange` can pass
  `null`, and "nothing chosen" is `null`, not `undefined` (that would make it uncontrolled). Menu items act on
  `onClick` (Base UI has no `onSelect`), a `DropdownMenuLabel` must sit in a `DropdownMenuGroup`, and a tab list
  needs `activateOnFocus` for the arrow keys to switch tabs. The woodmouse look (no shadows, solid `ring-2` focus
  rings, borders on popups, square checkboxes, the monospace `Badge` with `solid`/`signal` variants, the outline
  button's border hover, the boxed tab list) lives in `ui/`: keep it when updating a component from the registry.
- The SDK validates requests and responses against the spec (`validator: true`, zod): a backend answer
  that does not fit its OpenAPI document throws, so fix the backend's schema, not the frontend.
- Data fetching: TanStack Query (`useQuery`/`useMutation`). Tune defaults in
  `src/lib/query-client.ts`; wire real endpoints by replacing `queryFn`s in
  route components. A mutation whose component shows its own error passes `meta: { inline: true }`;
  any other failed mutation shows a toast (sonner). A 401 outside `/api/auth/*` means the session ended:
  the app goes to `/login` (`setSessionExpiredHandler` in `src/main.tsx`).
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
  (`../../lib/utils`, `./button`); `pnpm check` fails if one is left behind. The base styles list `cn` as a
  registry dependency, so the CLI installs an unrelated npm package called `cn` and imports from `"cn"`: remove
  the package (`pnpm remove cn`) and point the import at `../../lib/utils`.

- **View state belongs in the URL.** Anything that changes what a list, board, calendar, timeline or page shows
  (filters, sort, dates, zoom, search) is a validated search param (`validateSearch`; project views share
  `lib/project-search.ts` and `ProjectFilters`), never local state, so a link always reproduces the view.
  Write explicit values (not "today" or a default that can change); update with `replace: true`.

- Wiki editor: `PageEditor` picks live editing (`LiveBody`: Yjs + y-websocket + TipTap collaboration; the
  server saves) or, if the socket cannot connect, `ManualBody` (no autosave, Save button, version check).
  Mentions and page links save only `id` and `label`; their current titles come from `useReferences` via the
  `References` extension storage, never from node attributes (attributes sync to everyone and are saved).

- Single sign-on: the button is a plain `<a href={SSO_LOGIN_URL}>` (`lib/sso.ts`), not a generated-client
  call, because the browser itself has to go to the provider; it is the one exception to using the SDK. The
  callback comes back to `/login?error=<code>`; the messages in `lib/sso.ts` must cover the backend's
  `SsoError` codes (`routes/oidc.rs`), so keep the two lists in step.
- Rules of Hooks: call every hook before any early `return` (the lint has no react-hooks plugin to catch it).
- Ask for a confirmation with `useConfirm` (`src/components/common/confirm-dialog.tsx`), never `window.confirm`.

- Themes: colours live in `src/lib/themes.ts` (palettes in light and dark, turned into CSS at startup) and are
  read through the semantic tokens in `index.css`; never hardcode colours in components. `useTheme` (in
  `RootLayout`) applies the person's own choice for the workspace or else the workspace's, plus their light/dark
  mode; `--accent-color` (set from the chosen accent) overrides the palette's own accent. A new palette goes
  in the `PALETTES` list and the backend's `themes::PALETTES`.
