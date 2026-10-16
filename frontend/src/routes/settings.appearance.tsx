import { createFileRoute } from "@tanstack/react-router";

import { AppearanceSettings } from "../components/AccountPage";

export const Route = createFileRoute("/settings/appearance")({
  component: AppearanceSettings,
});
