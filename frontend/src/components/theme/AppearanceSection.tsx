import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Workspace } from "woodmouse-client";
import {
  getThemeOptions,
  listWorkspacesOptions,
  meOptions,
  meQueryKey,
  setMyThemeMutation,
  updateMeMutation,
} from "woodmouse-client/react-query";

import { effectiveChoice, paletteOf, type Mode } from "../../lib/themes";
import { Segmented } from "../Segmented";
import { ThemePicker, type ThemeValue } from "./ThemePicker";

const MODES: { value: Mode; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Device" },
];

/** Your light or dark mode, and for each workspace its theme or your own. */
export function AppearanceSection() {
  const queryClient = useQueryClient();
  const { data: me } = useQuery(meOptions());
  const { data: workspaces = [] } = useQuery(listWorkspacesOptions());
  const updateMe = useMutation({
    ...updateMeMutation(),
    meta: { inline: true },
    onSuccess: (user) => queryClient.setQueryData(meQueryKey(), user),
  });
  if (!me) return null;
  const mode = me.theme_mode as Mode;

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <span className="text-sm">Light or dark</span>
        <Segmented
          label="Light or dark"
          value={mode}
          options={MODES}
          onChange={(next) => updateMe.mutate({ body: { theme_mode: next } })}
        />
      </div>
      <div className="flex flex-col gap-2">
        <span className="text-sm">Your theme in each workspace</span>
        {workspaces.map((w) => (
          <WorkspaceTheme key={w.id} workspace={w} mode={mode} />
        ))}
      </div>
    </div>
  );
}

function WorkspaceTheme({ workspace, mode }: { workspace: Workspace; mode: Mode }) {
  const queryClient = useQueryClient();
  const { data: view } = useQuery(getThemeOptions({ path: { id: workspace.id } }));
  const save = useMutation({
    ...setMyThemeMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  if (!view) return null;

  const own = !!(view.mine.palette || view.mine.accent);
  const shown = effectiveChoice(view);
  const value: ThemeValue = { palette: shown.palette, accent: shown.accent };
  const put = (next: ThemeValue | null) =>
    save.mutate({
      path: { id: workspace.id },
      body: next ? { palette: next.palette, accent: next.accent } : {},
    });

  return (
    <div className="flex flex-col gap-3 border border-border bg-background p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-medium">{workspace.name}</span>
        <span className="text-xs text-muted-foreground">
          {own ? "Your own theme" : `Workspace theme: ${paletteOf(view.workspace.palette).label}`}
        </span>
      </div>
      <Segmented
        label={`Theme in ${workspace.name}`}
        value={own ? "own" : "workspace"}
        options={[
          { value: "workspace", label: "Use workspace theme" },
          { value: "own", label: "My own" },
        ]}
        onChange={(next) => put(next === "own" ? value : null)}
      />
      {own ? <ThemePicker value={value} mode={mode} onChange={put} /> : null}
    </div>
  );
}
