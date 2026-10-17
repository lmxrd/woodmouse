import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { CalendarLinkView, LinkScope } from "woodmouse-client";
import {
  listCalendarLinksOptions,
  revokeCalendarLinkMutation,
  revokeCalendarLinksMutation,
} from "woodmouse-client/react-query";

import { calendarFeedUrl } from "../../lib/api";
import { useFormatters } from "../../lib/useFormatters";
import { ConfirmDialog } from "../ConfirmDialog";
import { Button } from "../ui/button";
import { NewCalendarLink } from "./NewCalendarLink";

const SCOPE_LABEL: Record<LinkScope, string> = {
  user: "My tasks",
  project: "Project",
  workspace: "Workspace",
};

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success("Link copied");
  } catch {
    // Clipboard access needs a secure page; show the address so it can be copied by hand.
    window.prompt("Copy this link", text);
  }
}

/**
 * The calendar links you have made, with copy, add-to-calendar and revoke. `workspaceId` and `projectId`
 * narrow it to one workspace or project (the same component serves the account page, a workspace's
 * settings and a project's menu).
 */
export function CalendarLinks({
  workspaceId,
  projectId,
  scopes,
}: {
  workspaceId?: string;
  projectId?: string;
  /** What a new link may show here. */
  scopes: LinkScope[];
}) {
  const queryClient = useQueryClient();
  const { date, when } = useFormatters();
  const { data: links = [], isPending } = useQuery(
    listCalendarLinksOptions({ query: { workspace_id: workspaceId, project_id: projectId } }),
  );
  const refresh = () => queryClient.invalidateQueries();
  const revoke = useMutation(revokeCalendarLinkMutation());
  const revokeAll = useMutation(revokeCalendarLinksMutation());
  const [revoking, setRevoking] = useState<CalendarLinkView | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);
  const [creating, setCreating] = useState(false);

  const card = (link: CalendarLinkView) => {
    const url = calendarFeedUrl(link.path);
    const where =
      link.scope === "project"
        ? [link.project_name, link.workspace_name].filter(Boolean).join(" · ")
        : link.scope === "workspace"
          ? link.workspace_name
          : "All your workspaces";
    return (
      <li key={link.id} className="flex flex-col gap-2 border border-border bg-card px-3 py-2.5">
        <div className="flex items-start justify-between gap-2">
          <span className="min-w-0 break-words font-medium">{link.name}</span>
          <span className="label-mono shrink-0 text-[10px] text-muted-foreground">
            {SCOPE_LABEL[link.scope]}
          </span>
        </div>
        <p className="text-xs text-muted-foreground">
          {where} · made {date(link.created_at)} ·{" "}
          {link.last_used_at ? `last used ${when(link.last_used_at)}` : "never used"}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={() => void copy(url)}>
            Copy link
          </Button>
          <Button size="sm" variant="outline" asChild>
            {/* webcal:// opens the device's calendar app to subscribe */}
            <a href={url.replace(/^https?:/, "webcal:")}>Add to calendar</a>
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setRevoking(link)}>
            Revoke
          </Button>
        </div>
      </li>
    );
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Subscribe from a calendar app to see tasks with dates (ranges span their days, done tasks
        are marked). Links are read-only. Anyone who has one can see those tasks, so keep them
        private.
      </p>
      {!isPending && links.length === 0 ? (
        <p className="border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
          No calendar links yet.
        </p>
      ) : null}
      <ul className="flex flex-col gap-2">{links.map(card)}</ul>
      <div className="flex items-center justify-between gap-2">
        <Button
          variant="outline"
          size="sm"
          className="border-dashed"
          onClick={() => setCreating(true)}
        >
          + New link
        </Button>
        {links.length > 0 ? (
          <Button
            variant="ghost"
            size="sm"
            className="text-destructive"
            onClick={() => setConfirmAll(true)}
          >
            Revoke all
          </Button>
        ) : null}
      </div>

      <NewCalendarLink
        open={creating}
        onOpenChange={setCreating}
        workspaceId={workspaceId}
        projectId={projectId}
        scopes={scopes}
        onCreated={(link) => {
          void refresh();
          void copy(calendarFeedUrl(link.path));
        }}
      />
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => !open && setRevoking(null)}
        title={`Revoke "${revoking?.name ?? ""}"?`}
        description="The link stops working at once. Calendar apps that use it stop updating."
        confirmLabel="Revoke link"
        onConfirm={() => revoking && revoke.mutate({ path: { id: revoking.id } })}
      />
      <ConfirmDialog
        open={confirmAll}
        onOpenChange={setConfirmAll}
        title={`Revoke ${links.length === 1 ? "this link" : `all ${links.length} links`}?`}
        description="They stop working at once. Calendar apps that use them stop updating."
        confirmLabel="Revoke all"
        onConfirm={() =>
          revokeAll.mutate({ query: { workspace_id: workspaceId, project_id: projectId } })
        }
      />
    </div>
  );
}
