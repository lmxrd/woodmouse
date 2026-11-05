import { createFileRoute } from "@tanstack/react-router";

import { PasswordSection } from "../components/settings/AccountPage";
import { TwoFactorSection } from "../components/settings/TwoFactorSection";

export const Route = createFileRoute("/settings/password")({
  component: () => (
    <>
      <PasswordSection />
      <TwoFactorSection />
    </>
  ),
});
