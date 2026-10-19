import { createFileRoute, redirect } from "@tanstack/react-router";

/** The old address of the account page. */
export const Route = createFileRoute("/account")({
  beforeLoad: () => {
    throw redirect({ to: "/settings/account", replace: true });
  },
});
