import { Link } from "@tanstack/react-router";
import { useDraggable } from "@dnd-kit/core";
import type { TaskResponse } from "woodmouse-client";

import { cn } from "../../lib/utils";
import { isDone } from "../../lib/task";

/** How a task chip looks: late tasks are orange, completed ones struck through and dim. */
export function eventTone(task: TaskResponse, today: number, end: number): string {
  if (isDone(task)) return "border-l-muted-foreground bg-muted text-muted-foreground line-through";
  if (end < today) return "border-l-signal bg-signal/15";
  return "border-l-primary bg-accent";
}

/** A task on the calendar: drag it to another day to reschedule, click it to open it. */
export function CalendarEvent({
  task,
  today,
  end,
  workspaceId,
  projectId,
  editable,
  continuesLeft,
  continuesRight,
  className,
  style,
}: {
  task: TaskResponse;
  today: number;
  /** The last day the task occupies (for the late check). */
  end: number;
  workspaceId: string;
  projectId: string;
  editable: boolean;
  continuesLeft: boolean;
  continuesRight: boolean;
  className?: string;
  style?: React.CSSProperties;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: task.id,
    disabled: !editable,
  });
  return (
    <div
      ref={setNodeRef}
      style={style}
      {...listeners}
      {...attributes}
      className={cn(
        "flex min-w-0 items-center border-l-[3px] px-1.5 text-xs leading-5 touch-manipulation",
        eventTone(task, today, end),
        editable && "cursor-grab",
        isDragging && "opacity-40",
        continuesLeft && "border-l-0 pl-0.5",
        continuesRight && "pr-0.5",
        className,
      )}
    >
      {continuesLeft ? (
        <span aria-hidden className="mr-1 text-muted-foreground">
          ◂
        </span>
      ) : null}
      <Link
        to="/w/$workspaceId/p/$projectId/t/$taskId"
        params={{ workspaceId, projectId, taskId: task.id }}
        search={(prev) => prev}
        draggable={false}
        className="min-w-0 flex-1 truncate"
      >
        {task.title}
      </Link>
      {continuesRight ? (
        <span aria-hidden className="ml-1 text-muted-foreground">
          ▸
        </span>
      ) : null}
    </div>
  );
}
