import { Link } from "@tanstack/react-router";
import type { List, TaskResponse } from "woodmouse-client";

import { isDueNow } from "../../lib/dates";
import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import type { TaskSort } from "../../lib/projectSearch";
import { PRIORITY_BADGE, isHotPriority } from "../../lib/priority";
import { cn } from "../../lib/utils";
import { LabelTag } from "../LabelTag";
import { AvatarStack } from "../UserAvatar";
import { Checkbox } from "../ui/checkbox";
import { isDone } from "../../lib/task";

interface Props {
  tasks: TaskResponse[];
  lists: List[];
  workspaceId: string;
  projectId: string;
  sort: TaskSort | undefined;
  order: "asc" | "desc";
  onSort: (sort: TaskSort) => void;
  onToggleDone: (task: TaskResponse, done: boolean) => void;
}

function TaskLink({
  task,
  workspaceId,
  projectId,
  className,
}: Pick<Props, "workspaceId" | "projectId"> & { task: TaskResponse; className?: string }) {
  return (
    <Link
      to="/w/$workspaceId/p/$projectId/t/$taskId"
      params={{ workspaceId, projectId, taskId: task.id }}
      search={(prev) => prev}
      className={cn(isDone(task) && "text-muted-foreground line-through", className)}
    >
      {task.title}
    </Link>
  );
}

function Due({ task }: { task: TaskResponse }) {
  const { date } = useFormatters();
  if (task.due_date == null) return <span className="text-muted-foreground">—</span>;
  return (
    <span className={cn(isDueNow(task.due_date) && !isDone(task) && "font-medium text-signal")}>
      {date(task.due_date)}
    </span>
  );
}

/** Tasks as a table from `sm` up and as stacked rows on phones. */
export function TaskTable(props: Props) {
  const { tasks, lists, workspaceId, projectId, sort, order, onSort, onToggleDone } = props;
  const listName = (id: string | null | undefined) =>
    id ? (lists.find((l) => l.id === id)?.name ?? "") : "Inbox";
  const arrow = (column: TaskSort) => (sort === column ? (order === "asc" ? " ↑" : " ↓") : "");
  const header = (column: TaskSort, text: string) => (
    <th scope="col" className="px-2 py-2 text-left font-normal">
      <button
        type="button"
        className="label-mono hover:text-foreground"
        onClick={() => onSort(column)}
      >
        {text}
        {arrow(column)}
      </button>
    </th>
  );
  const plainHeader = (text: string) => (
    <th scope="col" className="label-mono px-2 py-2 text-left font-normal">
      {text}
    </th>
  );

  if (tasks.length === 0) {
    return <p className="p-6 text-sm text-muted-foreground">No tasks match these filters.</p>;
  }

  return (
    <>
      <table className="hidden w-full text-sm sm:table">
        <thead className="border-b border-border text-muted-foreground">
          <tr>
            <th className="w-10 py-2 pl-4" />
            {header("title", "Title")}
            {plainHeader("List")}
            {header("priority", "Priority")}
            {header("due_date", "Due")}
            {plainHeader("Who")}
          </tr>
        </thead>
        <tbody>
          {tasks.map((task) => (
            <tr key={task.id} className="border-b border-border hover:bg-card">
              <td className="py-2.5 pl-4">
                <Checkbox
                  aria-label={`Done: ${task.title}`}
                  checked={isDone(task)}
                  onCheckedChange={(done) => onToggleDone(task, done === true)}
                />
              </td>
              <td className="px-2 font-medium">
                <TaskLink task={task} workspaceId={workspaceId} projectId={projectId} />
                {task.labels.map((l) => (
                  <LabelTag
                    key={l.id}
                    name={l.name}
                    color={l.color}
                    className="ml-1.5 align-middle"
                  />
                ))}
              </td>
              <td className="px-2 text-muted-foreground">{listName(task.list_id)}</td>
              <td
                className={cn(
                  "px-2 font-mono text-[11px]",
                  isHotPriority(task.priority) ? "text-signal" : "text-muted-foreground",
                )}
              >
                {PRIORITY_BADGE[task.priority]}
              </td>
              <td className="px-2">
                <Due task={task} />
              </td>
              <td className="px-2 pr-4">
                <AvatarStack names={task.assignees.map(userLabel)} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <ul className="sm:hidden">
        {tasks.map((task) => (
          <li key={task.id} className="flex items-start gap-3 border-b border-border px-4 py-3">
            <Checkbox
              className="mt-0.5"
              aria-label={`Done: ${task.title}`}
              checked={isDone(task)}
              onCheckedChange={(done) => onToggleDone(task, done === true)}
            />
            <div className="min-w-0 flex-1">
              <TaskLink
                task={task}
                workspaceId={workspaceId}
                projectId={projectId}
                className="block font-medium"
              />
              <p className="mt-1 flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
                <Due task={task} />
                <span>· {listName(task.list_id)}</span>
                {isHotPriority(task.priority) ? (
                  <span className="text-signal">· {PRIORITY_BADGE[task.priority]}</span>
                ) : null}
              </p>
            </div>
            <AvatarStack names={task.assignees.map(userLabel)} />
          </li>
        ))}
      </ul>
    </>
  );
}
