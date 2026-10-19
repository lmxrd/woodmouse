import { useQuery } from "@tanstack/react-query";
import { listWorkspacesOptions } from "woodmouse-client/react-query";

import { CalendarLinks } from "../calendarlinks/CalendarLinks";
import { WorkspaceAppearance } from "../theme/WorkspaceAppearance";
import { Tabs, TabsList, TabsTrigger } from "../ui/tabs";
import { GeneralPanel } from "./GeneralPanel";
import { LabelsPanel } from "./LabelsPanel";
import { MembersPanel } from "./MembersPanel";
import { TransferPanel } from "./TransferPanel";

export type WorkspaceSettingsTab =
  | "members"
  | "labels"
  | "calendar"
  | "appearance"
  | "transfer"
  | "general";

/** The `tab` search param: anything unknown (and `members`, the default) is no param. */
export function parseSettingsTab(raw: unknown): WorkspaceSettingsTab | undefined {
  return raw === "labels" ||
    raw === "calendar" ||
    raw === "appearance" ||
    raw === "transfer" ||
    raw === "general"
    ? raw
    : undefined;
}

/**
 * A workspace's settings as tabs (members, labels, calendar links, appearance, general). Used by the
 * workspace's own settings page and, for the super admin, inside the site admin.
 */
export function WorkspaceSettings({
  workspaceId,
  tab = "members",
  onTab,
  heading,
}: {
  workspaceId: string;
  tab?: WorkspaceSettingsTab;
  onTab: (tab: WorkspaceSettingsTab | undefined) => void;
  /** Replaces the "Workspace" caption above the title. */
  heading?: string;
}) {
  const { data: workspaces } = useQuery(listWorkspacesOptions());
  const workspace = workspaces?.find((w) => w.id === workspaceId);

  return (
    <>
      <header className="border-b border-border px-4 pt-5 sm:px-6">
        <p className="label-mono text-muted-foreground">{heading ?? "Workspace"}</p>
        <h1 className="font-display text-2xl font-bold">{workspace?.name ?? " "} · Settings</h1>
        <Tabs
          value={tab}
          onValueChange={(next) => onTab(next === "members" ? undefined : parseSettingsTab(next))}
          className="mt-3"
        >
          <TabsList>
            <TabsTrigger value="members">Members</TabsTrigger>
            <TabsTrigger value="labels">Labels</TabsTrigger>
            <TabsTrigger value="calendar">Calendar</TabsTrigger>
            <TabsTrigger value="appearance">Appearance</TabsTrigger>
            <TabsTrigger value="transfer">Import & export</TabsTrigger>
            <TabsTrigger value="general">General</TabsTrigger>
          </TabsList>
        </Tabs>
      </header>
      {tab === "transfer" ? (
        <TransferPanel workspaceId={workspaceId} />
      ) : tab === "general" ? (
        <GeneralPanel workspaceId={workspaceId} />
      ) : tab === "labels" ? (
        <LabelsPanel workspaceId={workspaceId} />
      ) : tab === "appearance" ? (
        <WorkspaceAppearance workspaceId={workspaceId} />
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
