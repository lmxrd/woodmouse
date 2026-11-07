import { createFileRoute } from "@tanstack/react-router";

import { EmailTab } from "../components/admin/email-tab";

export const Route = createFileRoute("/settings/admin/email")({
  component: EmailTab,
});
