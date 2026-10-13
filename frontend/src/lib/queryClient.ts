import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Tune caching/refetch behavior project-wide here.
      staleTime: 30_000,
    },
  },
});
