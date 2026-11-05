import { createFileRoute } from "@tanstack/react-router";

import { AccountSection } from "../components/settings/AccountPage";
import { ApiKeysSection } from "../components/settings/ApiKeysSection";

export const Route = createFileRoute("/settings/account")({
  component: () => (
    <>
      <AccountSection />
      <ApiKeysSection />
    </>
  ),
});
