import { createFileRoute } from "@tanstack/react-router";

import { EmailTab } from "../components/admin/EmailTab";

export const Route = createFileRoute("/settings/admin/email")({
  component: EmailTab,
});
