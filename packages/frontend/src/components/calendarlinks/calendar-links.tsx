import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { CalendarLinkView, LinkScope } from "woodmouse-sdk";
import {
  listCalendarLinksOptions,
  revokeCalendarLinkMutation,
  revokeCalendarLinksMutation,
} from "woodmouse-sdk/react-query";

import { calendarFeedUrl } from "../../lib/api";
import { useFormatters } from "../../lib/use-formatters";
import { useConfirm } from "../common/confirm-dialog";
import { Button, buttonVariants } from "../ui/button";
import { NewCalendarLink } from "./new-calendar-link";

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
  const confirm = useConfirm();
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
          {/* webcal:// opens the device's calendar app to subscribe */}
          <a
            href={url.replace(/^https?:/, "webcal:")}
            className={buttonVariants({ size: "sm", variant: "outline" })}
          >
            Add to calendar
          </a>
          <Button
            size="sm"
            variant="ghost"
            onClick={async () => {
              if (
                await confirm({
                  title: `Revoke "${link.name}"?`,
                  description:
                    "The link stops working at once. Calendar apps that use it stop updating.",
                  action: "Revoke link",
                  destructive: true,
                })
              )
                revoke.mutate({ path: { id: link.id } });
            }}
          >
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
            onClick={async () => {
              if (
                await confirm({
                  title: `Revoke ${links.length === 1 ? "this link" : `all ${links.length} links`}?`,
                  description:
                    "They stop working at once. Calendar apps that use them stop updating.",
                  action: "Revoke all",
                  destructive: true,
                })
              )
                revokeAll.mutate({ query: { workspace_id: workspaceId, project_id: projectId } });
            }}
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
    </div>
  );
}
