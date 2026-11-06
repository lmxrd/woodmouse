import { createFileRoute } from "@tanstack/react-router";

import { CalendarSettings } from "../components/settings/account-page";

export const Route = createFileRoute("/settings/calendar")({
  component: CalendarSettings,
});
