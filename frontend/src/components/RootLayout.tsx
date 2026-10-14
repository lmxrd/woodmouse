import { Link, Outlet, useRouterState } from "@tanstack/react-router";

export function RootLayout() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  // The login page is a standalone sheet: no app navigation around it.
  if (pathname === "/login") {
    return (
      <main className="grid min-h-screen place-items-center bg-background px-4 py-10 text-foreground">
        <Outlet />
      </main>
    );
  }

  return (
    <div className="min-h-screen bg-background text-foreground">
      <nav className="border-b border-border bg-card">
        <div className="mx-auto flex max-w-4xl items-center gap-6 px-6 py-4">
          <span className="font-display font-bold">Woodmouse</span>
          <div className="flex gap-4 text-sm text-muted-foreground">
            <Link to="/" activeProps={{ className: "font-medium text-foreground" }}>
              Home
            </Link>
            <Link to="/about" activeProps={{ className: "font-medium text-foreground" }}>
              About
            </Link>
          </div>
        </div>
      </nav>
      <main className="mx-auto max-w-4xl px-6 py-8">
        <Outlet />
      </main>
    </div>
  );
}
