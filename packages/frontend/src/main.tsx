import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { toast } from "sonner";

// Configure the generated API client before any request fires.
import { setSessionExpiredHandler } from "./lib/api";
import { queryClient } from "./lib/query-client";
import { ConfirmProvider } from "./components/common/confirm-dialog";
import { Toaster } from "./components/ui/sonner";
import { router } from "./router";
import "./index.css";

setSessionExpiredHandler(() => {
  queryClient.clear();
  void router.navigate({ to: "/login" });
  toast.info("Your session expired. Log in again.", { id: "session-expired" });
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ConfirmProvider>
        <RouterProvider router={router} />
      </ConfirmProvider>
      <Toaster position="bottom-right" />
    </QueryClientProvider>
  </StrictMode>,
);
