import { createFileRoute } from "@tanstack/react-router";

import { MaintenanceTab } from "../components/admin/MaintenanceTab";

export const Route = createFileRoute("/settings/admin/maintenance")({
  component: () => (
    <>
      <h2 className="font-display text-xl font-bold">Maintenance</h2>
      <MaintenanceTab />
    </>
  ),
});
