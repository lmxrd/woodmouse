import { createFileRoute } from "@tanstack/react-router";

import { AccountSection } from "../components/settings/AccountPage";

export const Route = createFileRoute("/settings/account")({
  component: AccountSection,
});
