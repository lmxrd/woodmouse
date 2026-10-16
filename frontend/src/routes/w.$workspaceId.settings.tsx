import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { listWorkspacesOptions } from "woodmouse-client/react-query";

import { CalendarLinks } from "../components/calendarlinks/CalendarLinks";
import { LabelsPanel } from "../components/settings/LabelsPanel";
import { MembersPanel } from "../components/settings/MembersPanel";
import { Tabs, TabsList, TabsTrigger } from "../components/ui/tabs";

interface SettingsSearch {
  tab?: "members" | "labels" | "calendar";
}

export const Route = createFileRoute("/w/$workspaceId/settings")({
  validateSearch: (raw: Record<string, unknown>): SettingsSearch => ({
    tab: raw.tab === "labels" || raw.tab === "calendar" ? raw.tab : undefined,
  }),
  component: SettingsPage,
});

function SettingsPage() {
  const { workspaceId } = Route.useParams();
  const { tab = "members" } = Route.useSearch();
  const navigate = Route.useNavigate();
  const { data: workspaces } = useQuery(listWorkspacesOptions());
  const workspace = workspaces?.find((w) => w.id === workspaceId);

  return (
    <>
      <header className="border-b border-border px-4 pt-5 sm:px-6">
        <p className="label-mono text-muted-foreground">Workspace</p>
        <h1 className="font-display text-2xl font-bold">{workspace?.name ?? " "} · Settings</h1>
        <Tabs
          value={tab}
          onValueChange={(next) =>
            void navigate({
              search: { tab: next === "labels" || next === "calendar" ? next : undefined },
            })
          }
          className="mt-3"
        >
          <TabsList>
            <TabsTrigger value="members">Members</TabsTrigger>
            <TabsTrigger value="labels">Labels</TabsTrigger>
            <TabsTrigger value="calendar">Calendar</TabsTrigger>
          </TabsList>
        </Tabs>
      </header>
      {tab === "labels" ? (
        <LabelsPanel workspaceId={workspaceId} />
      ) : tab === "calendar" ? (
        <div className="mx-auto max-w-2xl p-4 sm:p-6">
          <CalendarLinks workspaceId={workspaceId} scopes={["workspace", "project"]} />
        </div>
      ) : (
        <MembersPanel workspaceId={workspaceId} />
      )}
    </>
  );
}
