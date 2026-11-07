import { MutationCache, QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { justLostSession } from "./api";

declare module "@tanstack/react-query" {
  interface Register {
    /** `inline: true` marks mutations whose component already shows its own error, so no toast. */
    mutationMeta: {
      inline?: boolean;
      /** `false` skips the refetch every successful mutation otherwise triggers (login, logout). */
      invalidate?: boolean;
    };
  }
}

/** The one query cache of the app. Routes use the SDK's query options and mutations through it. */
export const queryClient: QueryClient = new QueryClient({
  mutationCache: new MutationCache({
    // Any change can show up anywhere (counts, activity, trash), so a saved change refreshes every
    // query; this runs before (and is awaited by) the mutation's own `onSuccess`.
    onSuccess: (_data, _variables, _context, mutation): Promise<void> | undefined =>
      mutation.meta?.invalidate === false ? undefined : queryClient.invalidateQueries(),
    onError: (_error, _variables, _context, mutation) => {
      if (mutation.meta?.inline || justLostSession()) return;
      toast.error("Couldn't save that. Check your connection and try again.");
    },
  }),
  defaultOptions: {
    queries: {
      // A failed request shows at once. What was loaded stays fresh for 30 s, so moving between pages
      // does not refetch everything; switching back to the tab refreshes what others changed.
      retry: false,
      staleTime: 30_000,
    },
  },
});
