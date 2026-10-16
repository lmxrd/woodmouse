import { createFileRoute } from "@tanstack/react-router";

import { SettingsLayout } from "../components/settings/SettingsLayout";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/settings")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: SettingsLayout,
});
