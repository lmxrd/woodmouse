import { createFileRoute, redirect } from "@tanstack/react-router";

/** The old address of the site admin page. */
export const Route = createFileRoute("/admin")({
  beforeLoad: () => {
    throw redirect({ to: "/settings/admin/users", replace: true });
  },
});
