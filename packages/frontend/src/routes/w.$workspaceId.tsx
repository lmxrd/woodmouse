import { useEffect, useState } from "react";
import { createFileRoute, Outlet, useLocation } from "@tanstack/react-router";

import { MobileNav } from "../components/layout/MobileNav";
import { Sidebar } from "../components/layout/Sidebar";
import { SearchPalette } from "../components/search/SearchPalette";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/w/$workspaceId")({
  beforeLoad: ({ context }) => requireLogin(context.queryClient),
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspaceId } = Route.useParams();
  const [searching, setSearching] = useState(false);
  // The part of the app being shown (project, wiki, files, ...): changing it fades the new one in, while
  // moving within it (another task, another page) keeps the screen steady.
  const section = useLocation({ select: (l) => l.pathname.split("/").slice(3, 5).join("/") });
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
      <main
        key={section.startsWith("p/") ? section : section.split("/")[0]}
        className="min-w-0 animate-in fade-in duration-150"
      >
        <Outlet />
      </main>
      <SearchPalette workspaceId={workspaceId} open={searching} onOpenChange={setSearching} />
    </div>
  );
}
