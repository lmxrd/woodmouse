import { createFileRoute, Outlet } from "@tanstack/react-router";

import { MobileNav } from "../components/MobileNav";
import { Sidebar } from "../components/Sidebar";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/w/$workspaceId")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspaceId } = Route.useParams();
  return (
    <div className="min-h-screen md:grid md:grid-cols-[232px_minmax(0,1fr)]">
      <Sidebar workspaceId={workspaceId} className="hidden md:flex" />
      <MobileNav workspaceId={workspaceId} />
      <main className="min-w-0">
        <Outlet />
      </main>
    </div>
  );
}
