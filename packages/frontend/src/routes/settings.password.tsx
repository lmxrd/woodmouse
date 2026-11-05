import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { authStatusOptions } from "woodmouse-sdk/react-query";

import { PasswordSection } from "../components/settings/AccountPage";
import { TwoFactorSection } from "../components/settings/TwoFactorSection";

export const Route = createFileRoute("/settings/password")({
  component: PasswordSettings,
});

function PasswordSettings() {
  const { data: status } = useQuery(authStatusOptions());
  if (!status) return null;
  if (!status.password_login) {
    return (
      <p className="text-sm text-muted-foreground">
        You sign in with {status.sso ?? "single sign-on"}; passwords are turned off on this site.
      </p>
    );
  }
  return (
    <>
      <PasswordSection />
      <TwoFactorSection />
    </>
  );
}
