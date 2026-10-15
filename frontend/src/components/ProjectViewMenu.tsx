import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createViewMutation, listViewsOptions } from "woodmouse-client/react-query";

import type { ProjectSearch, ProjectView } from "../lib/projectSearch";
import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { ManageViewsDialog } from "./views/ManageViewsDialog";
import { SaveViewDialog } from "./views/SaveViewDialog";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

const VIEWS: { value: ProjectView; label: string }[] = [
  { value: "board", label: "Board" },
  { value: "list", label: "List" },
  { value: "calendar", label: "Calendar" },
  { value: "timeline", label: "Timeline" },
];

/** The settings a saved view keeps: what is set in the URL, except the date a calendar is showing. */
function paramsOf(search: ProjectSearch): Record<string, string> {
  return Object.fromEntries(
    Object.entries(search).filter(([k, v]) => k !== "date" && typeof v === "string" && v !== ""),
  ) as Record<string, string>;
}

const normalize = (params: Record<string, unknown>) =>
  JSON.stringify(
    Object.entries({ view: "board", ...params }).sort(([a], [b]) => a.localeCompare(b)),
  );

/**
 * Picks how the project's tasks are shown: the built-in views (add timeline or Gantt to `VIEWS`
 * later) and the project's saved views, plus saving the current settings under a name.
 */
export function ProjectViewMenu({
  value,
  onChange,
  projectId,
  workspaceId,
  search,
  onApply,
}: {
  value: ProjectView;
  onChange: (view: ProjectView) => void;
  projectId: string;
  workspaceId: string;
  search: ProjectSearch;
  /** Replace the view settings with a saved view's. */
  onApply: (params: Record<string, unknown>) => void;
}) {
  const queryClient = useQueryClient();
  const { canWrite } = useWorkspaceRole(workspaceId);
  const { data: views } = useQuery(listViewsOptions({ path: { id: projectId } }));
  const create = useMutation({
    ...createViewMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  const [saving, setSaving] = useState(false);
  const [managing, setManaging] = useState(false);

  const params = paramsOf(search);
  const active = views?.find((v) => normalize(v.params) === normalize(params));

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="outline" size="sm" aria-label="View">
            {active?.name ?? VIEWS.find((v) => v.value === value)?.label} ▾
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-56">
          <DropdownMenuRadioGroup
            value={active ? "" : value}
            onValueChange={(v) => onChange(v as ProjectView)}
          >
            {VIEWS.map((view) => (
              <DropdownMenuRadioItem key={view.value} value={view.value}>
                {view.label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
          {views && views.length > 0 ? (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel className="label-mono text-muted-foreground">
                Saved views
              </DropdownMenuLabel>
              {views.map((view) => (
                <DropdownMenuItem key={view.id} onSelect={() => onApply(view.params)}>
                  <span className="min-w-0 flex-1 truncate">
                    {view.id === active?.id ? "✓ " : ""}
                    {view.name}
                  </span>
                  <span className="label-mono text-[10px] text-muted-foreground">
                    {view.is_shared ? "Shared" : "Mine"}
                  </span>
                </DropdownMenuItem>
              ))}
            </>
          ) : null}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setSaving(true)}>
            Save current filters…
          </DropdownMenuItem>
          {views && views.length > 0 ? (
            <DropdownMenuItem onSelect={() => setManaging(true)}>Manage views…</DropdownMenuItem>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>
      <SaveViewDialog
        open={saving}
        onOpenChange={setSaving}
        canShare={canWrite}
        onSave={(name, isShared) =>
          create.mutate({ path: { id: projectId }, body: { name, params, is_shared: isShared } })
        }
      />
      <ManageViewsDialog
        open={managing}
        onOpenChange={setManaging}
        views={views ?? []}
        canShare={canWrite}
        currentParams={params}
      />
    </>
  );
}
