import type { KeyboardEventHandler } from "react";
import { Link } from "@tanstack/react-router";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { TaskResponse } from "woodmouse-sdk";

import { isDueNow } from "../../lib/dates";
import { PRIORITY_BADGE, isHotPriority } from "../../lib/priority";
import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { cn } from "../../lib/utils";
import { LabelTag } from "../common/LabelTag";
import { AvatarStack } from "../common/UserAvatar";
import { Checkbox } from "../ui/checkbox";
import { useQuery } from "@tanstack/react-query";
import { listSprintsOptions } from "woodmouse-sdk/react-query";

import { formatDuration } from "../../lib/duration";
import { isDone } from "../../lib/task";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

interface CardProps {
  task: TaskResponse;
  workspaceId: string;
  projectId: string;
  editable: boolean;
  onToggleCompleted: (task: TaskResponse, isCompleted: boolean) => void;
}

/** The card's look. The checkbox completes the task; the title link, stretched over the card, opens it. */
export function TaskCardBody({
  task,
  workspaceId,
  projectId,
  editable,
  onToggleCompleted,
}: CardProps) {
  const { dateShort } = useFormatters();
  const { data: sprints } = useQuery({
    ...listSprintsOptions({ path: { project_id: task.project_id } }),
    enabled: !!task.sprint_id,
  });
  const sprint = sprints?.find((s) => s.id === task.sprint_id);
  return (
    <>
      <Checkbox
        className="relative z-10 mt-0.5"
        aria-label={`Completed: ${task.title}`}
        checked={isDone(task)}
        disabled={!editable}
        onCheckedChange={(checked) => onToggleCompleted(task, checked === true)}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-start justify-between gap-2">
          <h3
            className={cn(
              "text-sm leading-snug font-medium",
              isDone(task) && "text-muted-foreground line-through",
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
        {task.subtask_total > 0 ? (
          <div
            className="h-[3px] bg-border"
            role="progressbar"
            aria-label="Subtasks completed"
            aria-valuenow={task.subtask_completed}
            aria-valuemax={task.subtask_total}
          >
            <div
              className="h-full bg-primary"
              style={{ width: `${(task.subtask_completed / task.subtask_total) * 100}%` }}
            />
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {task.due_date != null ? (
            <span
              className={cn(isDueNow(task.due_date) && !isDone(task) && "font-medium text-signal")}
            >
              {dateShort(task.due_date)}
            </span>
          ) : null}
          {task.sprint_id && sprint ? (
            <span className="label-mono max-w-28 truncate bg-muted px-1.5 py-0.5" title="Sprint">
              {sprint.name}
            </span>
          ) : null}
          {task.estimate_minutes != null || task.spent_minutes != null ? (
            <span
              className={cn(
                "font-mono",
                task.estimate_minutes != null &&
                  (task.spent_minutes ?? 0) > task.estimate_minutes &&
                  "text-signal",
              )}
              title="Time spent / estimate"
            >
              {formatDuration(task.spent_minutes ?? 0)}
              {task.estimate_minutes != null ? ` / ${formatDuration(task.estimate_minutes)}` : ""}
            </span>
          ) : null}
          {task.subtask_total > 0 ? (
            <span className="font-mono">
              {task.subtask_completed}/{task.subtask_total}
            </span>
          ) : null}
          {task.comment_count > 0 ? (
            <span className="font-mono">{plural(task.comment_count, "comment", "comments")}</span>
          ) : null}
          {task.attachment_count > 0 ? (
            <span className="font-mono">{plural(task.attachment_count, "file", "files")}</span>
          ) : null}
          <span className="ml-auto">
            <AvatarStack names={task.assignees.map(userLabel)} />
          </span>
        </div>
      </div>
    </>
  );
}

const cardClass = "relative flex gap-2.5 border border-border bg-card p-3 hover:border-foreground";

/** A card that can be dragged: by pointer or touch anywhere on it, by keyboard from its grip. */
export function SortableTaskCard(props: CardProps & { columnId: string }) {
  const { task, editable, columnId } = props;
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: task.id,
    data: { columnId },
    disabled: !editable,
  });
  // Space and Enter must keep working on the checkbox and the link, so only the grip handles keys.
  const { onKeyDown, ...pointerListeners } = listeners ?? {};

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cn(
        cardClass,
        editable && "cursor-grab touch-manipulation",
        isDone(task) && "bg-muted",
        isDragging && "opacity-40",
      )}
      {...pointerListeners}
    >
      <TaskCardBody {...props} />
      {editable ? (
        <button
          type="button"
          aria-label={`Move ${task.title}`}
          className="relative z-10 -mr-1 shrink-0 self-start px-0.5 text-muted-foreground opacity-40 outline-none hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
          {...attributes}
          onKeyDown={onKeyDown as KeyboardEventHandler<HTMLButtonElement> | undefined}
        >
          ⠿
        </button>
      ) : null}
    </div>
  );
}

/** The card as it floats under the pointer while dragging. */
export function TaskCardOverlay(props: CardProps) {
  return (
    <div
      className={cn(
        cardClass,
        "cursor-grabbing border-foreground shadow-md",
        isDone(props.task) && "bg-muted",
      )}
    >
      <TaskCardBody {...props} />
    </div>
  );
}
