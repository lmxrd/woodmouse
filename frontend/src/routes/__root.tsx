import { createRootRouteWithContext } from "@tanstack/react-router";
import { Link } from "@tanstack/react-router";

import { RootLayout } from "../components/RootLayout";
import type { RouterContext } from "../router";

function NotFound() {
  return (
    <div className="space-y-4">
      <h1 className="text-3xl font-bold tracking-tight">Page not found</h1>
      <p className="text-gray-600">The page you are looking for does not exist.</p>
      <Link to="/" className="text-blue-600 underline">
        Back to home
      </Link>
    </div>
  );
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
  notFoundComponent: NotFound,
});
