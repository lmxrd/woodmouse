import { createFileRoute } from "@tanstack/react-router";

import { AdminPage } from "../components/admin/AdminPage";
import { requireLogin } from "../lib/auth";

export type AdminTab = "users" | "workspaces" | "audit" | "maintenance";

interface AdminSearch {
  tab?: AdminTab;
  /** Audit log: only this action. */
  action?: string;
}

export const Route = createFileRoute("/admin")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  validateSearch: (raw: Record<string, unknown>): AdminSearch => ({
    tab:
      raw.tab === "workspaces" || raw.tab === "audit" || raw.tab === "maintenance"
        ? raw.tab
        : undefined,
    action: typeof raw.action === "string" && raw.action ? raw.action : undefined,
  }),
  component: AdminRoute,
});

function AdminRoute() {
  const { tab = "users", action } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <AdminPage
      tab={tab}
      action={action}
      onChange={(next) => void navigate({ search: next, replace: true })}
    />
  );
}
