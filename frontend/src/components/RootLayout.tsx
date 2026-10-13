import { Link, Outlet } from "@tanstack/react-router";

export function RootLayout() {
  return (
    <div className="min-h-screen bg-gray-50 text-gray-900">
      <nav className="border-b border-gray-200 bg-white">
        <div className="mx-auto flex max-w-4xl items-center gap-6 px-6 py-4">
          <span className="font-semibold">woodmouse</span>
          <div className="flex gap-4 text-sm">
            <Link to="/" activeProps={{ className: "font-medium" }}>
              Home
            </Link>
            <Link to="/about" activeProps={{ className: "font-medium" }}>
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
