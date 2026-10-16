import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  adminListDeletedWorkspacesOptions,
  adminRestoreWorkspaceMutation,
} from "woodmouse-client/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { Button } from "../ui/button";

/** Workspaces that were deleted; nothing in them is gone, so restoring brings everything back. */
export function WorkspacesTab() {
  const queryClient = useQueryClient();
  const { dateTime } = useFormatters();
  const { data: workspaces } = useQuery(adminListDeletedWorkspacesOptions());
  const restore = useMutation({
    ...adminRestoreWorkspaceMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });

  if (workspaces?.length === 0) {
    return (
      <p className="border border-dashed border-border px-4 py-6 text-muted-foreground">
        No deleted workspaces.
      </p>
    );
  }
  return (
    <section className="flex flex-col border border-border bg-card">
      {workspaces?.map((w) => (
        <div
          key={w.id}
          className="flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0"
        >
          <div className="min-w-0 flex-1">
            <p className="truncate font-medium">{w.name}</p>
            <p className="label-mono text-muted-foreground">Deleted {dateTime(w.deleted_at)}</p>
          </div>
          <Button size="sm" onClick={() => restore.mutate({ path: { id: w.id } })}>
            Restore
          </Button>
        </div>
      ))}
    </section>
  );
}
