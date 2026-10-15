import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";
import {
  createListMutation,
  createTaskMutation,
  listListsOptions,
  listTasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { formatDay, isDueNow } from "../lib/dates";
import { PRIORITY_BADGE, isHotPriority } from "../lib/priority";
import { cn } from "../lib/utils";
import { InlineAdd } from "./InlineAdd";
import { LabelTag } from "./LabelTag";
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
  const [dragOver, setDragOver] = useState<string | null>(null);

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
            <h2 className="label-mono flex justify-between border-b-2 border-foreground pb-1.5">
              <span>{list.name}</span>
              <span className="text-muted-foreground">{tasks.length}</span>
            </h2>
            {tasks.map((task) => (
              <TaskCard key={task.id} task={task} workspaceId={workspaceId} projectId={projectId} />
            ))}
            <InlineAdd
              label="Add task"
              placeholder="Task title"
              onSubmit={(title) =>
                createTask.mutate({ path: { list_id: list.id }, body: { title } })
              }
            />
          </section>
        );
      })}
      <InlineAdd
        className="w-[160px] shrink-0"
        label="Add list"
        placeholder="List name"
        onSubmit={(name) => createList.mutate({ path: { project_id: projectId }, body: { name } })}
      />
    </div>
  );
}

function TaskCard({
  task,
  workspaceId,
  projectId,
}: {
  task: TaskResponse;
  workspaceId: string;
  projectId: string;
}) {
  return (
    <Link
      to="/w/$workspaceId/p/$projectId/t/$taskId"
      params={{ workspaceId, projectId, taskId: task.id }}
      search={(prev) => prev}
      draggable
      onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
      className="flex cursor-grab flex-col gap-2 border border-border bg-card p-3 hover:border-foreground"
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
