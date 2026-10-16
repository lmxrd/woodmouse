import { createFileRoute } from "@tanstack/react-router";

import { NotificationSettings } from "../components/notifications/NotificationSettings";

export const Route = createFileRoute("/settings/notifications")({
  component: NotificationSettings,
});
