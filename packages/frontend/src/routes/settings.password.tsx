import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { authStatusOptions } from "woodmouse-sdk/react-query";

import { PasswordSection } from "../components/settings/AccountPage";
import { TwoFactorSection } from "../components/settings/TwoFactorSection";

export const Route = createFileRoute("/settings/password")({
  component: PasswordSettings,
});

/** The password (not with single sign-on only) and two-factor sign-in (always). */
function PasswordSettings() {
  const { data: status } = useQuery(authStatusOptions());
  if (!status) return null;
  return (
    <>
      {status.password_login ? <PasswordSection /> : null}
      <TwoFactorSection />
    </>
  );
}
