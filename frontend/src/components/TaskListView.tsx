import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import {
  listListsOptions,
  listSprintsOptions,
  listTasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { PRIORITIES } from "../lib/priority";
import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { taskFilterQuery, type ProjectSearch } from "../lib/projectSearch";
import { ProjectFilters } from "./ProjectFilters";
import { Segmented } from "./Segmented";
import { SprintGroups } from "./sprints/SprintGroups";
import { SprintFormDialog } from "./sprints/SprintDialogs";
import { AddTask } from "./task/AddTask";
import { Button } from "./ui/button";
import { TaskTable } from "./task/TaskTable";

const projectRoute = getRouteApi("/w/$workspaceId/p/$projectId");

/** The project's tasks as a filterable, sortable table. All state lives in the URL search params. */
export function TaskListView({
  workspaceId,
  projectId,
}: {
  workspaceId: string;
  projectId: string;
}) {
  const search = projectRoute.useSearch();
  const navigate = projectRoute.useNavigate();
  const setSearch = (patch: Partial<ProjectSearch>) =>
    void navigate({ search: (prev) => ({ ...prev, ...patch }), replace: true });

  const order = search.order ?? "asc";
  // Priority is the one sort the backend can't do (it's an enum); it is applied below.
  const serverSort =
    search.sort === "title" || search.sort === "due_date" ? search.sort : undefined;

  const { canWrite } = useWorkspaceRole(workspaceId);
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: sprints } = useQuery(listSprintsOptions({ path: { project_id: projectId } }));
  const grouped = search.group === "sprint";
  const [creating, setCreating] = useState(false);
  const { data: fetched } = useQuery(
    listTasksOptions({
      path: { workspace_id: workspaceId },
      query: {
        project_id: projectId,
        ...taskFilterQuery(search, "list"),
        sort: serverSort,
        order: serverSort ? order : undefined,
      },
    }),
  );
  const update = useMutation(updateTaskMutation());

  const tasks =
    search.sort === "priority" && fetched
      ? [...fetched].sort(
          (a, b) =>
            (PRIORITIES.indexOf(a.priority) - PRIORITIES.indexOf(b.priority)) *
            (order === "asc" ? 1 : -1),
        )
      : (fetched ?? []);

  const onSort = (column: NonNullable<ProjectSearch["sort"]>) =>
    setSearch(
      search.sort === column
        ? { order: order === "asc" ? "desc" : "asc" }
        : { sort: column, order: "asc" },
    );

  return (
    <>
      <ProjectFilters
        workspaceId={workspaceId}
        projectId={projectId}
        view="list"
        fields={["assignee", "label", "list", "sprint", "status"]}
        trailing={
          <span className="flex flex-wrap items-center gap-3">
            <span className="label-mono text-muted-foreground">
              {tasks.length} {tasks.length === 1 ? "task" : "tasks"}
            </span>
            <Segmented
              label="Group by"
              value={grouped ? "sprint" : "none"}
              onChange={(next) => setSearch({ group: next === "sprint" ? "sprint" : undefined })}
              options={[
                { value: "none", label: "No grouping" },
                { value: "sprint", label: "Sprints" },
              ]}
            />
            {grouped && canWrite ? (
              <Button size="sm" onClick={() => setCreating(true)}>
                New sprint
              </Button>
            ) : null}
            {grouped ? (
              <label className="flex items-center gap-1.5 text-xs">
                <input
                  type="checkbox"
                  checked={!!search.completed}
                  onChange={(e) => setSearch({ completed: e.target.checked ? true : undefined })}
                />
                Completed sprints
              </label>
            ) : null}
          </span>
        }
      />
      <SprintFormDialog projectId={projectId} open={creating} onOpenChange={setCreating} />

      {grouped ? (
        <SprintGroups
          tasks={tasks}
          sprints={sprints ?? []}
          lists={lists ?? []}
          workspaceId={workspaceId}
          projectId={projectId}
          canWrite={canWrite}
          showCompleted={!!search.completed}
          sort={search.sort}
          order={order}
          onSort={onSort}
          onToggleDone={(task, done) =>
            update.mutate({ path: { id: task.id }, body: { is_completed: done } })
          }
        />
      ) : (
        <TaskTable
          tasks={tasks}
          lists={lists ?? []}
          workspaceId={workspaceId}
          projectId={projectId}
          sort={search.sort}
          order={order}
          onSort={onSort}
          onToggleDone={(task, done) =>
            update.mutate({ path: { id: task.id }, body: { is_completed: done } })
          }
        />
      )}

      {canWrite ? (
        <AddTask projectId={projectId} lists={lists ?? []} defaultListId={search.list} />
      ) : null}
    </>
  );
}
