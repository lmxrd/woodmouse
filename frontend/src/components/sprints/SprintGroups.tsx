import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { List, SprintView, TaskResponse } from "woodmouse-client";
import { deleteSprintMutation } from "woodmouse-client/react-query";

import { formatDuration } from "../../lib/duration";
import type { TaskSort } from "../../lib/projectSearch";
import { isDone } from "../../lib/task";
import { useFormatters } from "../../lib/useFormatters";
import { ActionMenu } from "../ActionMenu";
import { ConfirmDialog } from "../ConfirmDialog";
import { TaskTable } from "../task/TaskTable";
import { CompleteSprintDialog, SprintFormDialog } from "./SprintDialogs";

/**
 * The list view grouped by sprint: the open sprints by start date, then the backlog; completed sprints
 * only when asked for. Each sprint shows its dates, progress and estimate, and can be edited, completed or
 * deleted.
 */
export function SprintGroups({
  tasks,
  sprints,
  lists,
  workspaceId,
  projectId,
  canWrite,
  showCompleted,
  sort,
  order,
  onSort,
  onToggleDone,
}: {
  tasks: TaskResponse[];
  sprints: SprintView[];
  lists: List[];
  workspaceId: string;
  projectId: string;
  canWrite: boolean;
  showCompleted: boolean;
  sort: TaskSort | undefined;
  order: "asc" | "desc";
  onSort: (column: TaskSort) => void;
  onToggleDone: (task: TaskResponse, done: boolean) => void;
}) {
  const { date } = useFormatters();
  const [editing, setEditing] = useState<SprintView | null>(null);
  const [completing, setCompleting] = useState<SprintView | null>(null);
  const [deleting, setDeleting] = useState<SprintView | null>(null);
  const remove = useMutation({
    ...deleteSprintMutation(),
  });

  const open = sprints.filter((s) => !s.completed_at);
  const shown = showCompleted ? sprints : open;
  const backlog = tasks.filter((t) => !t.sprint_id || !sprints.some((s) => s.id === t.sprint_id));
  const table = (group: TaskResponse[]) => (
    <TaskTable
      tasks={group}
      lists={lists}
      workspaceId={workspaceId}
      projectId={projectId}
      sort={sort}
      order={order}
      onSort={onSort}
      onToggleDone={onToggleDone}
      hideHeader
    />
  );

  return (
    <>
      {shown.map((s) => {
        const own = tasks.filter((t) => t.sprint_id === s.id);
        const unfinished = own.filter((t) => !isDone(t)).length;
        return (
          <section key={s.id} aria-label={s.name} className="border-b border-border">
            <header className="flex flex-wrap items-center gap-x-4 gap-y-1 bg-muted/50 px-4 py-2 sm:px-6">
              <h2 className="font-display font-bold">{s.name}</h2>
              <span className="label-mono text-muted-foreground">
                {date(s.start_date)} – {date(s.end_date)}
              </span>
              <span className="label-mono text-muted-foreground">
                {s.done_count}/{s.task_count} done
                {s.estimate_minutes > 0 ? ` · ${formatDuration(s.estimate_minutes)}` : ""}
              </span>
              {s.completed_at ? <span className="label-mono text-primary">completed</span> : null}
              {canWrite ? (
                <span className="ml-auto flex items-center gap-1">
                  {!s.completed_at ? (
                    <button
                      type="button"
                      className="label-mono px-2 py-1 hover:bg-accent"
                      onClick={() => setCompleting(s)}
                    >
                      Complete
                    </button>
                  ) : null}
                  <ActionMenu
                    label={`Actions for ${s.name}`}
                    items={[
                      { label: "Edit sprint…", onSelect: () => setEditing(s) },
                      {
                        label: "Delete sprint…",
                        destructive: true,
                        onSelect: () => setDeleting(s),
                      },
                    ]}
                  />
                </span>
              ) : null}
            </header>
            {s.goal ? <p className="px-4 py-1.5 text-sm sm:px-6">{s.goal}</p> : null}
            {own.length === 0 ? (
              <p className="px-4 py-3 text-sm text-muted-foreground sm:px-6">
                No tasks here. Give a task this sprint from its details.
              </p>
            ) : (
              table(own)
            )}
            {completing?.id === s.id ? (
              <CompleteSprintDialog
                sprint={s}
                others={open.filter((o) => o.id !== s.id)}
                unfinished={unfinished}
                open
                onOpenChange={(next) => !next && setCompleting(null)}
              />
            ) : null}
          </section>
        );
      })}
      <section aria-label="Backlog">
        <header className="flex items-center gap-4 bg-muted/50 px-4 py-2 sm:px-6">
          <h2 className="font-display font-bold">Backlog</h2>
          <span className="label-mono text-muted-foreground">
            {backlog.length} {backlog.length === 1 ? "task" : "tasks"}
          </span>
        </header>
        {backlog.length === 0 ? (
          <p className="px-4 py-3 text-sm text-muted-foreground sm:px-6">Nothing in the backlog.</p>
        ) : (
          table(backlog)
        )}
      </section>
      {editing ? (
        <SprintFormDialog
          projectId={projectId}
          sprint={editing}
          open
          onOpenChange={(next) => !next && setEditing(null)}
        />
      ) : null}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(next) => !next && setDeleting(null)}
        title={`Delete "${deleting?.name ?? ""}"?`}
        description="Its tasks go back to the backlog. You can restore the sprint from the trash, but not which tasks were in it."
        confirmLabel="Delete sprint"
        onConfirm={() => deleting && remove.mutate({ path: { id: deleting.id } })}
      />
    </>
  );
}
