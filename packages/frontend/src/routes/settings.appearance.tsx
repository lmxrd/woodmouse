import { createFileRoute } from "@tanstack/react-router";

import { AppearanceSettings } from "../components/settings/account-page";

export const Route = createFileRoute("/settings/appearance")({
  component: AppearanceSettings,
});
