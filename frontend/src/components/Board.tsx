import { useState } from "react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Priority, TaskResponse } from "woodmouse-client";
import {
  createListMutation,
  createTaskMutation,
  listListsOptions,
  listTasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { formatDay, isDueNow } from "../lib/dates";
import { cn } from "../lib/utils";
import { InlineAdd } from "./InlineAdd";
import { initials } from "./Sidebar";

const PRIORITY_LABEL: Record<Priority, string> = {
  low: "P1",
  medium: "P2",
  high: "P3",
  urgent: "P4",
};

/** One column per list of the project; dragging a card onto another column moves the task. */
export function Board({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const taskQueries = useQueries({
    queries: (lists ?? []).map((list) => listTasksOptions({ path: { list_id: list.id } })),
  });

  const refresh = () => queryClient.invalidateQueries();
  const createList = useMutation({ ...createListMutation(), onSuccess: refresh });
  const createTask = useMutation({ ...createTaskMutation(), onSuccess: refresh });
  const moveTask = useMutation({ ...updateTaskMutation(), onSuccess: refresh });
  const [dragOver, setDragOver] = useState<string | null>(null);

  return (
    <div className="flex items-start gap-4 p-6">
      {lists?.map((list, index) => {
        const tasks = taskQueries[index]?.data ?? [];
        return (
          <section
            key={list.id}
            aria-label={list.name}
            className={cn(
              "flex w-[232px] shrink-0 flex-col gap-2.5 pb-2",
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
              const moved = taskQueries.some((q) =>
                q.data?.some((t) => t.id === taskId && t.list_id === list.id),
              );
              if (taskId && !moved)
                moveTask.mutate({ path: { id: taskId }, body: { list_id: list.id } });
            }}
          >
            <h2 className="label-mono flex justify-between border-b-2 border-foreground pb-1.5">
              <span>{list.name}</span>
              <span className="text-muted-foreground">{tasks.length}</span>
            </h2>
            {tasks.map((task) => (
              <TaskCard key={task.id} task={task} />
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

function TaskCard({ task }: { task: TaskResponse }) {
  const hot = task.priority === "high" || task.priority === "urgent";
  return (
    <article
      draggable
      onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
      className="flex cursor-grab flex-col gap-2 border border-border bg-card p-3 hover:border-foreground"
    >
      <div className="flex items-center justify-between">
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
            className={cn("font-mono text-[10px]", hot ? "text-signal" : "text-muted-foreground")}
          >
            {PRIORITY_LABEL[task.priority]}
          </span>
        ) : null}
      </div>
      {task.labels.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {task.labels.map((label) => (
            <span
              key={label.id}
              className="bg-accent px-1.5 py-0.5 font-mono text-[10px] tracking-wide text-accent-foreground uppercase"
            >
              {label.name}
            </span>
          ))}
        </div>
      ) : null}
      <div className="flex items-center gap-3 text-xs text-muted-foreground">
        {task.due_date != null ? (
          <span className={cn(isDueNow(task.due_date) && !task.done && "font-medium text-signal")}>
            {formatDay(task.due_date)}
          </span>
        ) : null}
        <span className="ml-auto flex pl-1">
          {task.assignees.map((user) => (
            <span
              key={user.id}
              title={user.name}
              className="-ml-1 grid size-5 place-items-center rounded-full bg-secondary text-[9px] font-medium text-secondary-foreground ring-2 ring-card first:ml-0"
            >
              {initials(user.name)}
            </span>
          ))}
        </span>
      </div>
    </article>
  );
}
