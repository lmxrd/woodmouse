import { useDraggable } from "@dnd-kit/core";
import { Link } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";

import { cn } from "../../lib/utils";

function UnscheduledTask({
  task,
  workspaceId,
  projectId,
  editable,
}: {
  task: TaskResponse;
  workspaceId: string;
  projectId: string;
  editable: boolean;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: task.id,
    disabled: !editable,
  });
  return (
    <div
      ref={setNodeRef}
      {...listeners}
      {...attributes}
      className={cn(
        "border border-border bg-card px-2 py-1.5 text-sm touch-manipulation",
        editable && "cursor-grab",
        isDragging && "opacity-40",
      )}
    >
      <Link
        to="/w/$workspaceId/p/$projectId/t/$taskId"
        params={{ workspaceId, projectId, taskId: task.id }}
        search={(prev) => prev}
        draggable={false}
        className="block truncate"
      >
        {task.title}
      </Link>
    </div>
  );
}

/** Open tasks with no dates. Drag one onto a day to schedule it. */
export function UnscheduledPanel({
  tasks,
  workspaceId,
  projectId,
  editable,
}: {
  tasks: TaskResponse[];
  workspaceId: string;
  projectId: string;
  editable: boolean;
}) {
  return (
    <aside className="hidden w-48 shrink-0 flex-col gap-1.5 border-l border-border p-3 md:flex">
      <h2 className="label-mono text-muted-foreground">No due date · {tasks.length}</h2>
      {tasks.map((task) => (
        <UnscheduledTask
          key={task.id}
          task={task}
          workspaceId={workspaceId}
          projectId={projectId}
          editable={editable}
        />
      ))}
      {tasks.length === 0 ? (
        <p className="text-xs text-muted-foreground">Everything has a date.</p>
      ) : (
        <p className="label-mono text-[10px] text-muted-foreground">
          {editable ? "Drag onto a day to schedule" : ""}
        </p>
      )}
    </aside>
  );
}
