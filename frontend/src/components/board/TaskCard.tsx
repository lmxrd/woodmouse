import { Link } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";

import { formatDay, isDueNow } from "../../lib/dates";
import { PRIORITY_BADGE, isHotPriority } from "../../lib/priority";
import { cn } from "../../lib/utils";
import { LabelTag } from "../LabelTag";
import { AvatarStack } from "../UserAvatar";
import { Checkbox } from "../ui/checkbox";

/**
 * A task card: the checkbox completes it, the title link (stretched over the whole card) opens it,
 * and the card can be dragged between columns when `editable`.
 */
export function TaskCard({
  task,
  workspaceId,
  projectId,
  editable,
  onToggleCompleted,
}: {
  task: TaskResponse;
  workspaceId: string;
  projectId: string;
  editable: boolean;
  onToggleCompleted: (task: TaskResponse, isCompleted: boolean) => void;
}) {
  return (
    <div
      draggable={editable}
      onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
      className={cn(
        "relative flex gap-2.5 border border-border bg-card p-3 hover:border-foreground",
        editable && "cursor-grab",
        task.is_completed && "bg-muted",
      )}
    >
      <Checkbox
        className="relative z-10 mt-0.5"
        aria-label={`Completed: ${task.title}`}
        checked={task.is_completed}
        disabled={!editable}
        onCheckedChange={(checked) => onToggleCompleted(task, checked === true)}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-start justify-between gap-2">
          <h3
            className={cn(
              "text-sm leading-snug font-medium",
              task.is_completed && "text-muted-foreground line-through",
            )}
          >
            <Link
              to="/w/$workspaceId/p/$projectId/t/$taskId"
              params={{ workspaceId, projectId, taskId: task.id }}
              search={(prev) => prev}
              draggable={false}
              className="after:absolute after:inset-0"
            >
              {task.title}
            </Link>
          </h3>
          {task.priority !== "medium" ? (
            <span
              className={cn(
                "font-mono text-[10px]",
                isHotPriority(task.priority) ? "text-signal" : "text-muted-foreground",
              )}
            >
              {PRIORITY_BADGE[task.priority]}
            </span>
          ) : null}
        </div>
        {task.labels.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {task.labels.map((label) => (
              <LabelTag key={label.id} name={label.name} color={label.color} />
            ))}
          </div>
        ) : null}
        <div className="flex items-center gap-3 text-xs text-muted-foreground">
          {task.due_date != null ? (
            <span
              className={cn(
                isDueNow(task.due_date) && !task.is_completed && "font-medium text-signal",
              )}
            >
              {formatDay(task.due_date)}
            </span>
          ) : null}
          <span className="ml-auto">
            <AvatarStack names={task.assignees.map((u) => u.name)} />
          </span>
        </div>
      </div>
    </div>
  );
}
