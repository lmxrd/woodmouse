import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";

import { TaskDetail } from "../components/task/TaskDetail";

/** The same task view as a full page, for bookmarks and sharing. */
export const Route = createFileRoute("/w/$workspaceId/t/$taskId")({
  component: TaskPage,
});

function TaskPage() {
  const { workspaceId, taskId } = Route.useParams();
  const navigate = useNavigate();

  return (
    <div className="mx-auto max-w-3xl border-x border-b border-border bg-card sm:my-6 sm:border">
      <TaskDetail
        taskId={taskId}
        workspaceId={workspaceId}
        onDeleted={() => void navigate({ to: "/w/$workspaceId", params: { workspaceId } })}
        onOpenTask={(id) =>
          void navigate({ to: "/w/$workspaceId/t/$taskId", params: { workspaceId, taskId: id } })
        }
        header={({ projectId, listName }) => (
          <nav
            aria-label="Breadcrumb"
            className="label-mono flex items-center gap-2 border-b border-border px-5 py-3 text-muted-foreground"
          >
            {projectId ? (
              <Link
                to="/w/$workspaceId/p/$projectId"
                params={{ workspaceId, projectId }}
                className="hover:text-foreground"
              >
                ← Board
              </Link>
            ) : null}
            <span>/ {listName}</span>
          </nav>
        )}
      />
    </div>
  );
}
