import { MutationCache, QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { justLostSession } from "./api";

declare module "@tanstack/react-query" {
  interface Register {
    /** `inline: true` marks mutations whose component already shows its own error, so no toast. */
    mutationMeta: { inline?: boolean };
  }
}

export const queryClient = new QueryClient({
  mutationCache: new MutationCache({
    onError: (_error, _variables, _context, mutation) => {
      if (mutation.meta?.inline || justLostSession()) return;
      toast.error("Couldn't save that. Check your connection and try again.");
    },
  }),
  defaultOptions: {
    queries: {
      // Tune caching/refetch behavior project-wide here.
      staleTime: 30_000,
    },
  },
});
