import { createFileRoute, Outlet, redirect } from "@tanstack/react-router";
import { meOptions } from "woodmouse-client/react-query";

/** Everything under here is for the super admin; anyone else is sent back to their account. */
export const Route = createFileRoute("/settings/admin")({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meOptions());
    if (!me.super_admin) throw redirect({ to: "/settings/account", replace: true });
  },
  component: () => <Outlet />,
});
