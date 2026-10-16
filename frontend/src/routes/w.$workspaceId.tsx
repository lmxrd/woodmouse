import { useEffect, useState } from "react";
import { createFileRoute, Outlet } from "@tanstack/react-router";

import { MobileNav } from "../components/MobileNav";
import { Sidebar } from "../components/Sidebar";
import { SearchPalette } from "../components/search/SearchPalette";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/w/$workspaceId")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspaceId } = Route.useParams();
  const [searching, setSearching] = useState(false);
  // ⌘K (or Ctrl+K) opens the search from anywhere in the workspace.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearching((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="min-h-screen md:grid md:grid-cols-[232px_minmax(0,1fr)]">
      <Sidebar
        workspaceId={workspaceId}
        className="hidden md:flex"
        onSearch={() => setSearching(true)}
      />
      <MobileNav workspaceId={workspaceId} onSearch={() => setSearching(true)} />
      <main className="min-w-0">
        <Outlet />
      </main>
      <SearchPalette workspaceId={workspaceId} open={searching} onOpenChange={setSearching} />
    </div>
  );
}
