import { createFileRoute } from "@tanstack/react-router";

import { AuditTab } from "../components/admin/AuditTab";

interface Search {
  action?: string;
}

export const Route = createFileRoute("/settings/admin/audit")({
  validateSearch: (raw: Record<string, unknown>): Search => ({
    action: typeof raw.action === "string" && raw.action ? raw.action : undefined,
  }),
  component: Audit,
});

function Audit() {
  const { action } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <>
      <h2 className="font-display text-xl font-bold">Audit log</h2>
      <AuditTab
        action={action}
        onAction={(next) => void navigate({ search: { action: next }, replace: true })}
      />
    </>
  );
}
