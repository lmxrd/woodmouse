import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { TaskResponse } from "woodmouse-client";
import {
  getTaskRevisionOptions,
  listTaskRevisionsOptions,
  restoreTaskRevisionMutation,
} from "woodmouse-client/react-query";

import { HistoryPanel } from "../history/HistoryPanel";
import { Markdown } from "../common/Markdown";

function Preview({ revisionId }: { revisionId: string }) {
  const { data } = useQuery(getTaskRevisionOptions({ path: { id: revisionId } }));
  if (!data) return <p className="text-muted-foreground">Loading…</p>;
  return (
    <div className="flex flex-col gap-3">
      <h3 className="font-display text-xl font-medium">{data.title}</h3>
      {data.description.trim() ? (
        <Markdown>{data.description}</Markdown>
      ) : (
        <p className="text-muted-foreground">No description.</p>
      )}
    </div>
  );
}

/** Earlier titles and descriptions of a task, with restore. */
export function TaskHistory({
  taskId,
  open,
  onOpenChange,
  canRestore,
  onRestored,
}: {
  taskId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  canRestore: boolean;
  onRestored: (task: TaskResponse) => void;
}) {
  const queryClient = useQueryClient();
  const { data: revisions = [] } = useQuery({
    ...listTaskRevisionsOptions({ path: { id: taskId } }),
    enabled: open,
  });
  const restore = useMutation({
    ...restoreTaskRevisionMutation(),
    onSuccess: (task) => {
      void queryClient.invalidateQueries();
      onOpenChange(false);
      onRestored(task);
      toast.success("Version restored. What was there before is kept in the history.");
    },
  });
  return (
    <HistoryPanel
      open={open}
      onOpenChange={onOpenChange}
      title="Title and description history"
      revisions={revisions}
      canRestore={canRestore}
      restoring={restore.isPending}
      onRestore={(id) => restore.mutate({ path: { id } })}
      renderPreview={(id) => <Preview revisionId={id} />}
    />
  );
}
