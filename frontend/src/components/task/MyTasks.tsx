import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, getRouteApi } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";
import {
  listProjectsOptions,
  listTasksOptions,
  meOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { dueBucket, type DueBucket } from "../../lib/dates";
import { useFormatters } from "../../lib/useFormatters";
import type { TaskStatus } from "../../lib/projectSearch";
import { cn } from "../../lib/utils";
import { FilterSelect } from "../common/FilterSelect";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { isDone } from "../../lib/task";

const route = getRouteApi("/w/$workspaceId/my-tasks");

const GROUPS: { bucket: DueBucket; title: string; hot?: boolean }[] = [
  { bucket: "overdue", title: "Overdue", hot: true },
  { bucket: "today", title: "Today" },
  { bucket: "week", title: "This week" },
  { bucket: "later", title: "Later · No date" },
];

/** Everything assigned to the current user across the workspace, grouped by when it is due. */
export function MyTasks({ workspaceId }: { workspaceId: string }) {
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const status = search.status ?? "open";
  const setSearch = (patch: { status?: TaskStatus; project?: string }) =>
    void navigate({ search: (prev) => ({ ...prev, ...patch }), replace: true });

  const { date } = useFormatters();
  const { data: me } = useQuery(meOptions());
  const { data: projects } = useQuery(listProjectsOptions({ path: { workspace_id: workspaceId } }));
  const { data: tasks } = useQuery({
    ...listTasksOptions({
      path: { workspace_id: workspaceId },
      query: {
        assignee_id: me?.id,
        project_id: search.project,
        include_subtasks: true,
        is_completed: status === "all" ? undefined : status === "done",
      },
    }),
    enabled: !!me,
  });
  const update = useMutation({
    ...updateTaskMutation(),
  });
  const [showLater, setShowLater] = useState(false);

  const projectName = (id: string) => projects?.find((p) => p.id === id)?.name ?? "";
  const byDue = (a: TaskResponse, b: TaskResponse) =>
    (a.due_date ?? "9999").localeCompare(b.due_date ?? "9999") || a.title.localeCompare(b.title);
  const grouped = (bucket: DueBucket) =>
    (tasks ?? []).filter((t) => dueBucket(t.due_date) === bucket).sort(byDue);

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Assigned to you</p>
          <h1 className="font-display text-2xl font-bold">My tasks</h1>
        </div>
        <div className="flex flex-wrap gap-2">
          <FilterSelect
            label="Status"
            anyLabel="Open"
            value={status === "open" ? undefined : status}
            options={[
              { value: "done", label: "Completed" },
              { value: "all", label: "All" },
            ]}
            onChange={(next) => setSearch({ status: next as TaskStatus | undefined })}
          />
          <FilterSelect
            label="Project"
            anyLabel="Any project"
            value={search.project}
            options={(projects ?? []).map((p) => ({ value: p.id, label: p.name }))}
            onChange={(project) => setSearch({ project })}
          />
        </div>
      </header>

      {tasks && tasks.length === 0 ? (
        <p className="p-6 text-sm text-muted-foreground">
          Nothing assigned to you{status === "open" ? " right now" : ""}.
        </p>
      ) : null}

      {GROUPS.map(({ bucket, title, hot }) => {
        const rows = grouped(bucket);
        if (rows.length === 0) return null;
        const collapsed = bucket === "later" && !showLater;
        return (
          <section key={bucket} aria-label={title}>
            <h2 className="label-mono flex items-center justify-between border-t-2 border-foreground px-4 py-2 sm:px-6">
              <span className={hot ? "text-signal" : undefined}>{title}</span>
              <span className="flex items-center gap-2 text-muted-foreground">
                {rows.length}
                {bucket === "later" ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    onClick={() => setShowLater((v) => !v)}
                  >
                    {showLater ? "Hide" : "Show"}
                  </Button>
                ) : null}
              </span>
            </h2>
            {collapsed ? null : (
              <ul>
                {rows.map((task) => (
                  <li
                    key={task.id}
                    className="flex items-center gap-3 border-b border-border bg-card px-4 py-2.5 sm:px-6"
                  >
                    <Checkbox
                      aria-label={`Completed: ${task.title}`}
                      checked={isDone(task)}
                      onCheckedChange={(checked) =>
                        update.mutate({
                          path: { id: task.id },
                          body: { is_completed: checked === true },
                        })
                      }
                    />
                    <Link
                      to="/w/$workspaceId/t/$taskId"
                      params={{ workspaceId, taskId: task.id }}
                      className={cn(
                        "min-w-0 flex-1 truncate text-sm font-medium",
                        isDone(task) && "text-muted-foreground line-through",
                      )}
                    >
                      {task.title}
                    </Link>
                    <span className="label-mono hidden text-muted-foreground sm:inline">
                      {projectName(task.project_id)}
                    </span>
                    {task.due_date != null ? (
                      <span
                        className={cn(
                          "w-24 text-right text-xs",
                          bucket === "overdue" || bucket === "today"
                            ? "font-medium text-signal"
                            : "text-muted-foreground",
                        )}
                      >
                        {date(task.due_date)}
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </>
  );
}
