import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { TrashItem } from "woodmouse-client";
import { listTrashOptions, restoreItemMutation } from "woodmouse-client/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { Button } from "../ui/button";

const KIND_LABEL: Record<TrashItem["kind"], string> = {
  project: "Project",
  list: "List",
  task: "Task",
  label: "Label",
  comment: "Comment",
  view: "View",
  page: "Page",
  sprint: "Sprint",
  file: "File",
};

function detail(item: TrashItem): string {
  if (item.child_count === 0) return "";
  const noun =
    { project: "task", list: "task", task: "subtask", page: "subpage" }[item.kind as string] ??
    "item";
  return ` · with ${item.child_count} ${noun}${item.child_count === 1 ? "" : "s"}`;
}

/** Everything deleted in the workspace. Deleting is always soft: items can be restored from here. */
export function TrashPage({ workspaceId }: { workspaceId: string }) {
  const { when } = useFormatters();
  const { data: items, isError } = useQuery({
    ...listTrashOptions({ path: { id: workspaceId } }),
    retry: false,
  });
  const [failed, setFailed] = useState<string | null>(null);
  const restore = useMutation({
    ...restoreItemMutation(),
    meta: { inline: true },
    onMutate: () => setFailed(null),
    onError: (_e, vars) => setFailed(vars.path.item_id),
  });

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-2 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Workspace</p>
          <h1 className="font-display text-2xl font-bold">Trash</h1>
        </div>
        <p className="label-mono text-muted-foreground">Nothing is permanently deleted</p>
      </header>

      {isError ? (
        <p className="p-6 text-sm text-muted-foreground">
          Only members with write access can see the trash.
        </p>
      ) : null}
      {items && items.length === 0 ? (
        <p className="p-6 text-sm text-muted-foreground">The trash is empty.</p>
      ) : null}

      <ul>
        {items?.map((item) => (
          <li
            key={`${item.kind}-${item.id}`}
            className="flex flex-wrap items-center gap-3 border-b border-border bg-card px-4 py-3 sm:px-6"
          >
            <span className="label-mono w-20 bg-muted px-2 py-0.5 text-center text-[10px] text-muted-foreground">
              {KIND_LABEL[item.kind]}
            </span>
            <div className="min-w-0 flex-1 basis-56">
              <p className="truncate text-sm">{item.name}</p>
              <p className="label-mono text-muted-foreground">
                {item.context ? `${item.context} · ` : ""}
                {item.deleted_by ? `deleted by ${userLabel(item.deleted_by)} · ` : "deleted · "}
                {when(item.deleted_at)}
                {detail(item)}
              </p>
              {failed === item.id ? (
                <p role="alert" className="text-xs text-destructive">
                  Couldn't restore it. Restore the project, list or parent task it belongs to first.
                </p>
              ) : null}
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={restore.isPending}
              onClick={() =>
                restore.mutate({ path: { id: workspaceId, kind: item.kind, item_id: item.id } })
              }
            >
              Restore
            </Button>
          </li>
        ))}
      </ul>
    </>
  );
}
