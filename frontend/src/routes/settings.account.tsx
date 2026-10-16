import { createFileRoute } from "@tanstack/react-router";

import { AccountSection } from "../components/AccountPage";

export const Route = createFileRoute("/settings/account")({
  component: AccountSection,
});
