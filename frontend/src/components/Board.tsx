import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { List, TaskResponse } from "woodmouse-client";
import {
  createListMutation,
  createTaskMutation,
  deleteListMutation,
  listListsOptions,
  listTasksOptions,
  updateListMutation,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { formatDay, isDueNow } from "../lib/dates";
import { PRIORITY_BADGE, isHotPriority } from "../lib/priority";
import { cn } from "../lib/utils";
import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { ActionMenu } from "./ActionMenu";
import { ConfirmDialog } from "./ConfirmDialog";
import { InlineAdd } from "./InlineAdd";
import { LabelTag } from "./LabelTag";
import { RenameInput } from "./RenameInput";
import { AvatarStack } from "./UserAvatar";

/** One column per list of the project; dragging a card onto another column moves the task. */
export function Board({ projectId, workspaceId }: { projectId: string; workspaceId: string }) {
  const queryClient = useQueryClient();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: allTasks } = useQuery(
    listTasksOptions({ path: { workspace_id: workspaceId }, query: { project_id: projectId } }),
  );

  const refresh = () => queryClient.invalidateQueries();
  const createList = useMutation({ ...createListMutation(), onSuccess: refresh });
  const createTask = useMutation({ ...createTaskMutation(), onSuccess: refresh });
  const moveTask = useMutation({ ...updateTaskMutation(), onSuccess: refresh });
  const renameList = useMutation({ ...updateListMutation(), onSuccess: refresh });
  const deleteList = useMutation({ ...deleteListMutation(), onSuccess: refresh });
  const { canWrite } = useWorkspaceRole(workspaceId);
  const [dragOver, setDragOver] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<List | null>(null);

  const deletingCount = allTasks?.filter((t) => t.list_id === deleting?.id).length ?? 0;
  const deleteDescription =
    deletingCount === 0
      ? "This can't be undone."
      : `This also deletes its ${deletingCount} ${deletingCount === 1 ? "task" : "tasks"}. This can't be undone.`;

  return (
    <div className="flex snap-x snap-mandatory scroll-px-4 items-start gap-4 overflow-x-auto p-4 sm:snap-none sm:p-6">
      {lists?.map((list) => {
        const tasks = allTasks?.filter((t) => t.list_id === list.id) ?? [];
        return (
          <section
            key={list.id}
            aria-label={list.name}
            className={cn(
              "flex w-[78vw] shrink-0 snap-start flex-col gap-2.5 pb-2 sm:w-[232px]",
              dragOver === list.id && "bg-accent/50",
            )}
            onDragOver={(e) => {
              e.preventDefault();
              setDragOver(list.id);
            }}
            onDragLeave={() => setDragOver(null)}
            onDrop={(e) => {
              e.preventDefault();
              setDragOver(null);
              const taskId = e.dataTransfer.getData("text/plain");
              const moved = tasks.some((t) => t.id === taskId);
              if (taskId && !moved)
                moveTask.mutate({ path: { id: taskId }, body: { list_id: list.id } });
            }}
          >
            <div className="flex items-center gap-1 border-b-2 border-foreground pb-1">
              {renaming === list.id ? (
                <RenameInput
                  label={`Rename ${list.name}`}
                  value={list.name}
                  className="h-7 flex-1"
                  onCancel={() => setRenaming(null)}
                  onSave={(name) => {
                    renameList.mutate({ path: { id: list.id }, body: { name } });
                    setRenaming(null);
                  }}
                />
              ) : (
                <h2 className="label-mono flex min-w-0 flex-1 justify-between gap-2">
                  <span className="truncate">{list.name}</span>
                  <span className="text-muted-foreground">{tasks.length}</span>
                </h2>
              )}
              {canWrite && renaming !== list.id ? (
                <ActionMenu
                  label={`Actions for ${list.name}`}
                  items={[
                    { label: "Rename list", onSelect: () => setRenaming(list.id) },
                    { label: "Delete list…", destructive: true, onSelect: () => setDeleting(list) },
                  ]}
                />
              ) : null}
            </div>
            {tasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                workspaceId={workspaceId}
                projectId={projectId}
                draggable={canWrite}
              />
            ))}
            {canWrite ? (
              <InlineAdd
                label="Add task"
                placeholder="Task title"
                onSubmit={(title) =>
                  createTask.mutate({ path: { list_id: list.id }, body: { title } })
                }
              />
            ) : null}
          </section>
        );
      })}
      {canWrite ? (
        <InlineAdd
          className="w-[160px] shrink-0"
          label="Add list"
          placeholder="List name"
          onSubmit={(name) =>
            createList.mutate({ path: { project_id: projectId }, body: { name } })
          }
        />
      ) : null}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`Delete "${deleting?.name ?? ""}"?`}
        description={deleteDescription}
        confirmLabel="Delete list"
        onConfirm={() => deleting && deleteList.mutate({ path: { id: deleting.id } })}
      />
    </div>
  );
}

function TaskCard({
  task,
  workspaceId,
  projectId,
  draggable,
}: {
  task: TaskResponse;
  workspaceId: string;
  projectId: string;
  draggable: boolean;
}) {
  return (
    <Link
      to="/w/$workspaceId/p/$projectId/t/$taskId"
      params={{ workspaceId, projectId, taskId: task.id }}
      search={(prev) => prev}
      draggable={draggable}
      onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
      className={cn(
        "flex flex-col gap-2 border border-border bg-card p-3 hover:border-foreground",
        draggable && "cursor-grab",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <h3
          className={cn(
            "text-sm leading-snug font-medium",
            task.done && "text-muted-foreground line-through",
          )}
        >
          {task.title}
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
          <span className={cn(isDueNow(task.due_date) && !task.done && "font-medium text-signal")}>
            {formatDay(task.due_date)}
          </span>
        ) : null}
        <span className="ml-auto">
          <AvatarStack names={task.assignees.map((u) => u.name)} />
        </span>
      </div>
    </Link>
  );
}
