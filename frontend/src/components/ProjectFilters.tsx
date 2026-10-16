import { useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import {
  listLabelsOptions,
  listListsOptions,
  listMembersOptions,
  listSprintsOptions,
} from "woodmouse-client/react-query";

import {
  BACKLOG,
  DEFAULT_STATUS,
  type ProjectSearch,
  type ProjectView,
  type TaskStatus,
} from "../lib/projectSearch";
import { userLabel } from "../lib/user";
import { FilterSelect } from "./FilterSelect";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

const projectRoute = getRouteApi("/w/$workspaceId/p/$projectId");

const STATUS_LABEL: Record<TaskStatus, string> = { open: "Open", done: "Completed", all: "All" };

export type FilterField = "assignee" | "label" | "list" | "sprint" | "status";

/**
 * The filter dropdowns every project view shares. The choices live in the URL (`assignee`, `label`, `list`,
 * `status`), so a copied link shows the same tasks; `fields` picks the ones that make sense for a view.
 */
export function ProjectFilters({
  workspaceId,
  projectId,
  view,
  fields,
  trailing,
}: {
  workspaceId: string;
  projectId: string;
  view: ProjectView;
  fields: FilterField[];
  /** Shown at the end of the bar, e.g. a task count. */
  trailing?: React.ReactNode;
}) {
  const search = projectRoute.useSearch();
  const navigate = projectRoute.useNavigate();
  const setSearch = (patch: Partial<ProjectSearch>) =>
    void navigate({ search: (prev) => ({ ...prev, ...patch }), replace: true });
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: sprints } = useQuery({
    ...listSprintsOptions({ path: { project_id: projectId } }),
    enabled: fields.includes("sprint"),
  });
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const { data: labels } = useQuery(listLabelsOptions({ path: { workspace_id: workspaceId } }));
  const defaultStatus = DEFAULT_STATUS[view];

  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2.5 sm:px-6">
      {fields.includes("assignee") ? (
        <FilterSelect
          label="Assignee"
          anyLabel="Anyone"
          value={search.assignee}
          options={(members ?? []).map((m) => ({ value: m.user.id, label: userLabel(m.user) }))}
          onChange={(assignee) => setSearch({ assignee })}
        />
      ) : null}
      {fields.includes("label") ? (
        <FilterSelect
          label="Label"
          anyLabel="Any label"
          value={search.label}
          options={(labels ?? []).map((l) => ({ value: l.id, label: l.name }))}
          onChange={(label) => setSearch({ label })}
        />
      ) : null}
      {fields.includes("list") ? (
        <FilterSelect
          label="List"
          anyLabel="Any list"
          value={search.list}
          options={(lists ?? []).map((l) => ({ value: l.id, label: l.name }))}
          onChange={(list) => setSearch({ list })}
        />
      ) : null}
      {fields.includes("sprint") ? (
        <FilterSelect
          label="Sprint"
          anyLabel="Any sprint"
          value={search.sprint}
          options={[
            { value: BACKLOG, label: "Backlog (no sprint)" },
            ...(sprints ?? []).map((s) => ({ value: s.id, label: s.name })),
          ]}
          onChange={(sprint) => setSearch({ sprint })}
        />
      ) : null}
      {fields.includes("status") ? (
        <Select
          value={search.status ?? defaultStatus}
          onValueChange={(next) =>
            setSearch({ status: next === defaultStatus ? undefined : (next as TaskStatus) })
          }
        >
          <SelectTrigger
            size="sm"
            aria-label="Status"
            className={
              search.status && search.status !== defaultStatus
                ? "border-foreground"
                : "text-muted-foreground"
            }
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(Object.keys(STATUS_LABEL) as TaskStatus[]).map((s) => (
              <SelectItem key={s} value={s}>
                {STATUS_LABEL[s]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}
      {trailing ? <span className="ml-auto">{trailing}</span> : null}
    </div>
  );
}
