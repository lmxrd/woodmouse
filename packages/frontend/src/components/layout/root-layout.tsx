import { Outlet, useParams } from "@tanstack/react-router";

import { useDetectTimezone } from "../../lib/use-detect-timezone";
import { useTheme } from "../../lib/use-theme";

/** Bare frame: the login sheet and the app shell (see routes/w.$workspaceId.tsx) bring their own layout. */
export function RootLayout() {
  const { workspaceId } = useParams({ strict: false });
  useTheme(workspaceId);
  useDetectTimezone();
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Outlet />
    </div>
  );
}
