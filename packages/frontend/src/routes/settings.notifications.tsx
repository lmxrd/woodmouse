import { createFileRoute } from "@tanstack/react-router";

import { NotificationSettings } from "../components/notifications/notification-settings";

export const Route = createFileRoute("/settings/notifications")({
  component: NotificationSettings,
});
