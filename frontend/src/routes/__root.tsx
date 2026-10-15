import { createRootRouteWithContext } from "@tanstack/react-router";
import { Link } from "@tanstack/react-router";

import { RootLayout } from "../components/RootLayout";
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
  component: RootLayout,
  notFoundComponent: NotFound,
});
