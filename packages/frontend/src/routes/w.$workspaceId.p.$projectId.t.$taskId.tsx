import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";

import { ResponsiveDialog } from "../components/common/ResponsiveDialog";
import { TaskDetail } from "../components/task/TaskDetail";
import { Button } from "../components/ui/button";

/** The task as a dialog over its project's board. */
export const Route = createFileRoute("/w/$workspaceId/p/$projectId/t/$taskId")({
  component: TaskDialog,
});

function TaskDialog() {
  const { workspaceId, projectId, taskId } = Route.useParams();
  const navigate = useNavigate();
  const close = () =>
    void navigate({
      to: "/w/$workspaceId/p/$projectId",
      params: { workspaceId, projectId },
      search: (prev) => prev,
    });

  return (
    <ResponsiveDialog open onOpenChange={(open) => !open && close()} title="Task details">
      <TaskDetail
        taskId={taskId}
        workspaceId={workspaceId}
        onDeleted={close}
        onOpenTask={(id) =>
          void navigate({
            to: "/w/$workspaceId/p/$projectId/t/$taskId",
            params: { workspaceId, projectId, taskId: id },
            search: (prev) => prev,
            replace: true,
          })
        }
        header={({ listName }) => (
          <div className="flex items-center justify-between gap-2 border-b border-border px-5 py-2">
            <span className="label-mono truncate text-muted-foreground">{listName}</span>
            <span className="flex shrink-0 items-center gap-1">
              <Button asChild variant="ghost" size="xs">
                <Link to="/w/$workspaceId/t/$taskId" params={{ workspaceId, taskId }}>
                  Open as page
                </Link>
              </Button>
              <Button type="button" variant="ghost" size="xs" onClick={close}>
                Close
              </Button>
            </span>
          </div>
        )}
      />
    </ResponsiveDialog>
  );
}
