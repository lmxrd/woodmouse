import { createFileRoute } from "@tanstack/react-router";

import { AccountPage } from "../components/AccountPage";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/account")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: AccountPage,
});
