import { createFileRoute } from "@tanstack/react-router";

import { AccountSection } from "../components/settings/account-page";
import { ApiKeysSection } from "../components/settings/api-keys-section";

export const Route = createFileRoute("/settings/account")({
  component: () => (
    <>
      <AccountSection />
      <ApiKeysSection />
    </>
  ),
});
