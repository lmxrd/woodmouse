import { createFileRoute } from "@tanstack/react-router";

import { CalendarSettings } from "../components/AccountPage";

export const Route = createFileRoute("/settings/calendar")({
  component: CalendarSettings,
});
