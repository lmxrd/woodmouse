import { redirect, type useNavigate } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-sdk";
import { meOptions, meQueryKey } from "woodmouse-sdk/react-query";

/** Route guard: unauthenticated visitors land on the login page. */
export async function requireLogin(queryClient: QueryClient) {
  await queryClient.ensureQueryData(meOptions()).catch(() => {
    throw redirect({ to: "/login" });
  });
}

/**
 * After a successful login, bootstrap or invite acceptance: forget any earlier user and go home, where
 * the route guard loads the new one (`/auth/me` says more about them than the login's answer).
 */
export function finishLogin(
  queryClient: QueryClient,
  navigate: ReturnType<typeof useNavigate>,
  _user: PublicUser,
) {
  queryClient.removeQueries({ queryKey: meQueryKey() });
  void navigate({ to: "/" });
}
