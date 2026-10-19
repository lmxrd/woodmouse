import { createFileRoute } from "@tanstack/react-router";

import { PasswordSection } from "../components/settings/AccountPage";

export const Route = createFileRoute("/settings/password")({
  component: () => <PasswordSection />,
});
