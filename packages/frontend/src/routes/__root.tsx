import { createRootRouteWithContext, Link, redirect } from "@tanstack/react-router";
import { authStatusOptions, meOptions } from "woodmouse-sdk/react-query";

import { RootLayout } from "../components/layout/root-layout";
import type { RouterContext } from "../router";

function NotFound() {
  return (
    <div className="space-y-4 p-8">
      <h1 className="font-display text-3xl font-bold">Page not found</h1>
      <p className="text-muted-foreground">The page you are looking for does not exist.</p>
      <Link to="/" className="text-primary underline">
        Back to home
      </Link>
    </div>
  );
}

export const Route = createRootRouteWithContext<RouterContext>()({
  // Every page goes through here: signed-out → /login (which is also the first-run setup), but for the
  // invite links.
  beforeLoad: async ({ location, context }) => {
    // The signed-in user is null while nobody is (`/api/auth/me` answers 401). Both stay cached until a
    // sign-in or sign-out clears them (`finishLogin`, the logout).
    const status = await context.queryClient.ensureQueryData(authStatusOptions());
    const user = status.bootstrap_required
      ? null
      : await context.queryClient.ensureQueryData(meOptions()).catch(() => null);
    const session = { ...status, user };
    const path = location.pathname;
    const open = path === "/login" || path.startsWith("/invite/");
    if (!session.user && !open) throw redirect({ to: "/login" });
    if (session.user && path === "/login") throw redirect({ to: "/" });
    return { session };
  },
  component: RootLayout,
  notFoundComponent: NotFound,
});
