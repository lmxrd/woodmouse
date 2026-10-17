import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { TaskResponse } from "woodmouse-client";
import {
  createSubtaskMutation,
  listSubtasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { cn } from "../../lib/utils";
import { InlineAdd } from "../InlineAdd";
import { AvatarStack } from "../UserAvatar";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { isDone } from "../../lib/task";

/** Subtasks of a task at any depth, as an indented tree with progress and inline adding. */
export function SubtaskTree({
  task,
  editable,
  onOpenTask,
}: {
  task: TaskResponse;
  editable: boolean;
  onOpenTask: (taskId: string) => void;
}) {
  const { data: subtasks } = useQuery(listSubtasksOptions({ path: { id: task.id } }));
  const create = useMutation(createSubtaskMutation());
  const update = useMutation(updateTaskMutation());
  const [hideCompleted, setHideCompleted] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [addingUnder, setAddingUnder] = useState<string | null>(null);

  const childrenOf = (parentId: string) => (subtasks ?? []).filter((t) => t.parent_id === parentId);
  const toggleCollapsed = (id: string) =>
    setCollapsed((set) => {
      const next = new Set(set);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const total = task.subtask_total;
  if (total === 0 && !editable) return null;

  const rows = (parentId: string, depth: number): React.ReactNode =>
    childrenOf(parentId)
      .filter((t) => !(hideCompleted && isDone(t)))
      .map((child) => (
        <SubtaskRow
          key={child.id}
          task={child}
          depth={depth}
          editable={editable}
          isCollapsed={collapsed.has(child.id)}
          hasChildren={childrenOf(child.id).length > 0}
          onToggleCollapsed={() => toggleCollapsed(child.id)}
          onToggleCompleted={(done) =>
            update.mutate({ path: { id: child.id }, body: { is_completed: done } })
          }
          onOpen={() => onOpenTask(child.id)}
          onAddChild={() => setAddingUnder(child.id)}
        >
          {addingUnder === child.id ? (
            <div style={{ paddingLeft: (depth + 1) * 22 + 22 }} className="py-1.5">
              <InlineAdd
                label="Add subtask"
                placeholder="Subtask title"
                onSubmit={(title) => {
                  create.mutate({ path: { id: child.id }, body: { title } });
                  setAddingUnder(null);
                }}
              />
            </div>
          ) : null}
          {collapsed.has(child.id) ? null : rows(child.id, depth + 1)}
        </SubtaskRow>
      ));

  return (
    <section aria-label="Subtasks" className="flex flex-col gap-1">
      <div className="flex items-center justify-between gap-3">
        <h2 className="label-mono text-muted-foreground">
          Subtasks{total > 0 ? ` · ${task.subtask_completed} of ${total}` : ""}
        </h2>
        {total > 0 ? (
          <span className="flex items-center gap-3">
            <span className="h-1 w-24 bg-border" aria-hidden>
              <span
                className="block h-1 bg-primary"
                style={{ width: `${(task.subtask_completed / total) * 100}%` }}
              />
            </span>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => setHideCompleted((v) => !v)}
            >
              {hideCompleted ? "Show completed" : "Hide completed"}
            </Button>
          </span>
        ) : null}
      </div>
      <div>{rows(task.id, 0)}</div>
      {editable ? (
        <InlineAdd
          className="mt-1"
          label="Add subtask"
          placeholder="Subtask title"
          onSubmit={(title) => create.mutate({ path: { id: task.id }, body: { title } })}
        />
      ) : null}
    </section>
  );
}

function SubtaskRow({
  task,
  depth,
  editable,
  isCollapsed,
  hasChildren,
  onToggleCollapsed,
  onToggleCompleted,
  onOpen,
  onAddChild,
  children,
}: {
  task: TaskResponse;
  depth: number;
  editable: boolean;
  isCollapsed: boolean;
  hasChildren: boolean;
  onToggleCollapsed: () => void;
  onToggleCompleted: (isCompleted: boolean) => void;
  onOpen: () => void;
  onAddChild: () => void;
  children: React.ReactNode;
}) {
  const { date } = useFormatters();
  return (
    <>
      <div
        className="flex items-center gap-2 border-b border-border py-1.5"
        style={{ paddingLeft: depth * 22 }}
      >
        {hasChildren ? (
          <button
            type="button"
            aria-label={isCollapsed ? "Expand subtasks" : "Collapse subtasks"}
            aria-expanded={!isCollapsed}
            className="w-4 shrink-0 text-xs text-muted-foreground"
            onClick={onToggleCollapsed}
          >
            {isCollapsed ? "▸" : "▾"}
          </button>
        ) : (
          <span className="w-4 shrink-0" />
        )}
        <Checkbox
          aria-label={`Completed: ${task.title}`}
          checked={isDone(task)}
          disabled={!editable}
          onCheckedChange={(checked) => onToggleCompleted(checked === true)}
        />
        <button
          type="button"
          onClick={onOpen}
          className={cn(
            "min-w-0 flex-1 truncate text-left text-sm hover:underline",
            isDone(task) && "text-muted-foreground line-through",
          )}
        >
          {task.title}
        </button>
        {task.subtask_total > 0 ? (
          <span className="label-mono text-muted-foreground">
            {task.subtask_completed}/{task.subtask_total}
          </span>
        ) : null}
        {task.due_date != null ? (
          <span className="text-xs text-muted-foreground">{date(task.due_date)}</span>
        ) : null}
        <AvatarStack names={task.assignees.map(userLabel)} />
        {editable ? (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Add a subtask under ${task.title}`}
            onClick={onAddChild}
          >
            +
          </Button>
        ) : null}
      </div>
      {children}
    </>
  );
}
