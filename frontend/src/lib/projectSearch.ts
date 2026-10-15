export type ProjectView = "board" | "list";
export type TaskStatus = "open" | "done" | "all";
export type TaskSort = "title" | "due_date" | "priority";

/** URL search params of the project route. Absent means default (board, open tasks, list order). */
export interface ProjectSearch {
  view?: ProjectView;
  status?: TaskStatus;
  assignee?: string;
  label?: string;
  list?: string;
  sort?: TaskSort;
  order?: "asc" | "desc";
}

const oneOf = <T extends string>(value: unknown, allowed: readonly T[]): T | undefined =>
  allowed.find((a) => a === value);
const text = (value: unknown): string | undefined =>
  typeof value === "string" && value ? value : undefined;

/** Drops anything unknown or invalid, so a hand-edited URL can't break the page. */
export function validateProjectSearch(raw: Record<string, unknown>): ProjectSearch {
  return {
    view: oneOf(raw.view, ["board", "list"]),
    status: oneOf(raw.status, ["open", "done", "all"]),
    assignee: text(raw.assignee),
    label: text(raw.label),
    list: text(raw.list),
    sort: oneOf(raw.sort, ["title", "due_date", "priority"]),
    order: oneOf(raw.order, ["asc", "desc"]),
  };
}
