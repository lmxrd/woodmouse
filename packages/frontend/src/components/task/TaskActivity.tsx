import { useQuery } from "@tanstack/react-query";
import { listActivityOptions } from "woodmouse-sdk/react-query";

import { ActivityItem } from "../activity/ActivityItem";

/** The activity feed of one task. */
export function TaskActivity({ taskId, workspaceId }: { taskId: string; workspaceId: string }) {
  const { data: entries } = useQuery(
    listActivityOptions({ path: { id: workspaceId }, query: { task_id: taskId } }),
  );
  if (entries && entries.length === 0) {
    return <p className="text-sm text-muted-foreground">Nothing has happened yet.</p>;
  }
  return (
    <div className="-mx-5 border-t border-border">
      {entries?.map((entry) => (
        <ActivityItem key={entry.id} entry={entry} workspaceId={workspaceId} />
      ))}
    </div>
  );
}
