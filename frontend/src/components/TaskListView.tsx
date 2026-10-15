import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import {
  createTaskMutation,
  listLabelsOptions,
  listListsOptions,
  listMembersOptions,
  listTasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { PRIORITIES } from "../lib/priority";
import type { ProjectSearch, TaskStatus } from "../lib/projectSearch";
import { FilterSelect } from "./FilterSelect";
import { InlineAdd } from "./InlineAdd";
import { TaskTable } from "./task/TaskTable";

const projectRoute = getRouteApi("/w/$workspaceId/p/$projectId");

const STATUS_OPTIONS: { value: TaskStatus; label: string }[] = [
  { value: "open", label: "Open" },
  { value: "done", label: "Done" },
  { value: "all", label: "All" },
];

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

  const status = search.status ?? "open";
  const order = search.order ?? "asc";
  // Priority is the one sort the backend can't do (it's an enum); it is applied below.
  const serverSort =
    search.sort === "title" || search.sort === "due_date" ? search.sort : undefined;

  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const { data: labels } = useQuery(listLabelsOptions({ path: { workspace_id: workspaceId } }));
  const { data: fetched } = useQuery(
    listTasksOptions({
      path: { workspace_id: workspaceId },
      query: {
        project_id: projectId,
        list_id: search.list,
        assignee_id: search.assignee,
        label_id: search.label,
        done: status === "all" ? undefined : status === "done",
        sort: serverSort,
        order: serverSort ? order : undefined,
      },
    }),
  );
  const update = useMutation({ ...updateTaskMutation(), onSuccess: refresh });
  const create = useMutation({ ...createTaskMutation(), onSuccess: refresh });

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
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2.5 sm:px-6">
        <FilterSelect
          label="Assignee"
          anyLabel="Anyone"
          value={search.assignee}
          options={(members ?? []).map((m) => ({ value: m.user.id, label: m.user.name }))}
          onChange={(assignee) => setSearch({ assignee })}
        />
        <FilterSelect
          label="Label"
          anyLabel="Any label"
          value={search.label}
          options={(labels ?? []).map((l) => ({ value: l.id, label: l.name }))}
          onChange={(label) => setSearch({ label })}
        />
        <FilterSelect
          label="List"
          anyLabel="Any list"
          value={search.list}
          options={(lists ?? []).map((l) => ({ value: l.id, label: l.name }))}
          onChange={(list) => setSearch({ list })}
        />
        <FilterSelect
          label="Status"
          anyLabel="Open"
          value={status === "open" ? undefined : status}
          options={STATUS_OPTIONS.filter((o) => o.value !== "open")}
          onChange={(next) => setSearch({ status: next as TaskStatus | undefined })}
        />
        <span className="label-mono ml-auto text-muted-foreground">
          {tasks.length} {tasks.length === 1 ? "task" : "tasks"}
        </span>
      </div>

      <TaskTable
        tasks={tasks}
        lists={lists ?? []}
        workspaceId={workspaceId}
        projectId={projectId}
        sort={search.sort}
        order={order}
        onSort={onSort}
        onToggleDone={(task, done) => update.mutate({ path: { id: task.id }, body: { done } })}
      />

      {lists && lists.length > 0 ? (
        <div className="flex items-center gap-3 px-4 py-3 sm:px-6">
          <InlineAdd
            className="w-full max-w-sm"
            label={`Add task to ${(lists.find((l) => l.id === search.list) ?? lists[0]).name}`}
            placeholder="Task title"
            onSubmit={(title) =>
              create.mutate({
                path: { list_id: (lists.find((l) => l.id === search.list) ?? lists[0]).id },
                body: { title },
              })
            }
          />
        </div>
      ) : null}
    </>
  );
}
