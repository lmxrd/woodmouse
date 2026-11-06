import { createFileRoute } from "@tanstack/react-router";

import { SsoTab } from "../components/admin/sso-tab";

export const Route = createFileRoute("/settings/admin/sso")({
  component: SsoTab,
});
