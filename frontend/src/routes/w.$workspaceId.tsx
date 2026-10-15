import { createFileRoute, Outlet } from "@tanstack/react-router";

import { Sidebar } from "../components/Sidebar";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/w/$workspaceId")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspaceId } = Route.useParams();
  return (
    <div className="grid min-h-screen grid-cols-[232px_minmax(0,1fr)]">
      <Sidebar workspaceId={workspaceId} />
      <main className="min-w-0 overflow-x-auto">
        <Outlet />
      </main>
    </div>
  );
}
