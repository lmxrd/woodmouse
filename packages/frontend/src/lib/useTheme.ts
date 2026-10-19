import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { getThemeOptions, meOptions } from "woodmouse-sdk/react-query";

import { applyTheme, effectiveChoice, readCachedTheme, type Mode } from "./themes";

/**
 * Show the right theme: the person's own choice for this workspace or else the workspace's, in their light
 * or dark mode. Outside a workspace (login, account) the last theme used is kept.
 */
export function useTheme(workspaceId?: string) {
  // Only reads what the app already loaded for the signed-in user; never asks (this also runs on the login page).
  const { data: me } = useQuery({ ...meOptions(), enabled: false });
  const { data: view } = useQuery({
    ...getThemeOptions({ path: { id: workspaceId ?? "" } }),
    enabled: !!workspaceId,
  });
  const cached = readCachedTheme();
  const mode: Mode = (me?.theme_mode as Mode | undefined) ?? cached.mode;
  const choice = view ? effectiveChoice(view) : { palette: cached.palette, accent: cached.accent };
  const { palette, accent } = choice;

  useEffect(() => {
    applyTheme({ palette, accent, mode });
    if (mode !== "system") return;
    // Follow the device while it switches between light and dark.
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const follow = () => applyTheme({ palette, accent, mode });
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, [palette, accent, mode]);
}
