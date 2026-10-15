import { redirect } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import { meOptions } from "woodmouse-client/react-query";

/** Route guard: unauthenticated visitors land on the login page. */
export async function requireLogin(queryClient: QueryClient) {
  await queryClient.ensureQueryData(meOptions()).catch(() => {
    throw redirect({ to: "/login" });
  });
}
