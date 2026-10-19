import { useMutation, useQuery } from "@tanstack/react-query";
import { getThemeOptions, setWorkspaceThemeMutation } from "woodmouse-sdk/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { paletteOf, readCachedTheme } from "../../lib/themes";
import { ThemePicker, type ThemeValue } from "./ThemePicker";

/** The workspace's palette and accent: the default for everyone who has not chosen their own. */
export function WorkspaceAppearance({ workspaceId }: { workspaceId: string }) {
  const { isAdmin } = useWorkspaceRole(workspaceId);
  const { data: view } = useQuery(getThemeOptions({ path: { id: workspaceId } }));
  const save = useMutation({
    ...setWorkspaceThemeMutation(),
  });
  if (!view) return null;

  const value: ThemeValue = {
    palette: paletteOf(view.workspace.palette).id,
    accent: view.workspace.accent ?? null,
  };
  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-4 p-4 sm:p-6">
      <p className="text-sm text-muted-foreground">
        {isAdmin
          ? "The default look for everyone in this workspace. Members can use their own instead from their account page."
          : "Set by the workspace's admins. You can use your own look from your account page."}
      </p>
      <ThemePicker
        value={value}
        mode={readCachedTheme().mode}
        disabled={!isAdmin}
        onChange={(next) =>
          save.mutate({
            path: { id: workspaceId },
            body: { palette: next.palette, accent: next.accent },
          })
        }
      />
    </div>
  );
}
