import type { useNavigate } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-sdk";
import { authStatusQueryKey, meQueryKey } from "woodmouse-sdk/react-query";

/**
 * After a successful login, bootstrap or invite acceptance: forget any earlier user and go home, where
 * the root route loads the new one (`/auth/me` says more about them than the login's answer).
 */
export function finishLogin(
  queryClient: QueryClient,
  navigate: ReturnType<typeof useNavigate>,
  _user: PublicUser,
) {
  queryClient.removeQueries({ queryKey: meQueryKey() });
  queryClient.removeQueries({ queryKey: authStatusQueryKey() });
  void navigate({ to: "/" });
}
