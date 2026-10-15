import { Outlet } from "@tanstack/react-router";

/** Bare frame: the login sheet and the app shell (see routes/w.$workspaceId.tsx) bring their own layout. */
export function RootLayout() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Outlet />
    </div>
  );
}
