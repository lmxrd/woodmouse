import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { meOptions } from "woodmouse-client/react-query";

import { Tabs, TabsList, TabsTrigger } from "../ui/tabs";
import { AuditTab } from "./AuditTab";
import { MaintenanceTab } from "./MaintenanceTab";
import { UsersTab } from "./UsersTab";
import { WorkspacesTab } from "./WorkspacesTab";

type Tab = "users" | "workspaces" | "audit" | "maintenance";

/** The site admin: only the super admin gets in. The view (tab, audit filter) lives in the URL. */
export function AdminPage({
  tab,
  action,
  onChange,
}: {
  tab: Tab;
  action: string | undefined;
  onChange: (search: { tab?: Tab; action?: string }) => void;
}) {
  const { data: me } = useQuery(meOptions());
  if (!me) return null;
  if (!me.super_admin) {
    return <p className="p-6 text-muted-foreground">Not found.</p>;
  }
  return (
    <main className="mx-auto flex max-w-4xl flex-col gap-6 px-4 py-8 sm:px-6">
      <header className="flex flex-col gap-1">
        <Link to="/" className="label-mono text-muted-foreground hover:text-foreground">
          ← Back
        </Link>
        <h1 className="font-display text-3xl font-bold">Site admin</h1>
        <Tabs
          value={tab}
          onValueChange={(next) =>
            onChange({ tab: next === "users" ? undefined : (next as Tab), action })
          }
          className="mt-3"
        >
          <TabsList>
            <TabsTrigger value="users">Users</TabsTrigger>
            <TabsTrigger value="workspaces">Workspaces</TabsTrigger>
            <TabsTrigger value="audit">Audit log</TabsTrigger>
            <TabsTrigger value="maintenance">Maintenance</TabsTrigger>
          </TabsList>
        </Tabs>
      </header>
      {tab === "workspaces" ? (
        <WorkspacesTab />
      ) : tab === "audit" ? (
        <AuditTab action={action} onAction={(a) => onChange({ tab: "audit", action: a })} />
      ) : tab === "maintenance" ? (
        <MaintenanceTab />
      ) : (
        <UsersTab meId={me.id} />
      )}
    </main>
  );
}
