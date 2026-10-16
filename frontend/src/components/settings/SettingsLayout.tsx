import { useQuery } from "@tanstack/react-query";
import { Link, Outlet } from "@tanstack/react-router";
import { meOptions } from "woodmouse-client/react-query";

import { cn } from "../../lib/utils";

const link =
  "whitespace-nowrap px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground md:block";
const active = "bg-accent font-medium text-foreground";

/**
 * The settings shell: its own sidebar (a row of links on phones) with the person's account sections and,
 * for the super admin, the site admin sections. The selected section fills the rest.
 */
export function SettingsLayout() {
  const { data: me } = useQuery(meOptions());
  return (
    <div className="min-h-screen md:grid md:grid-cols-[232px_minmax(0,1fr)]">
      <aside className="flex flex-col gap-3 border-b border-border bg-card p-4 md:border-r md:border-b-0">
        <Link to="/" className="label-mono text-muted-foreground hover:text-foreground">
          ← Back
        </Link>
        <h1 className="font-display text-xl font-bold">Settings</h1>
        <nav aria-label="Settings" className="flex gap-1 overflow-x-auto md:flex-col md:gap-0">
          <Link to="/settings/account" className={link} activeProps={{ className: active }}>
            Account
          </Link>
          <Link to="/settings/appearance" className={link} activeProps={{ className: active }}>
            Appearance
          </Link>
          <Link to="/settings/calendar" className={link} activeProps={{ className: active }}>
            Calendar links
          </Link>
          <Link to="/settings/password" className={link} activeProps={{ className: active }}>
            Password
          </Link>
          {me?.super_admin ? (
            <>
              <p className={cn("label-mono px-3 pt-3 text-muted-foreground max-md:hidden")}>
                Site admin
              </p>
              <Link to="/settings/admin/users" className={link} activeProps={{ className: active }}>
                Users
              </Link>
              <Link
                to="/settings/admin/workspaces"
                className={link}
                activeProps={{ className: active }}
              >
                Workspaces
              </Link>
              <Link to="/settings/admin/audit" className={link} activeProps={{ className: active }}>
                Audit log
              </Link>
              <Link
                to="/settings/admin/maintenance"
                className={link}
                activeProps={{ className: active }}
              >
                Maintenance
              </Link>
            </>
          ) : null}
        </nav>
      </aside>
      <main className="mx-auto flex w-full min-w-0 max-w-3xl flex-col gap-6 px-4 py-8 sm:px-6">
        <Outlet />
      </main>
    </div>
  );
}
