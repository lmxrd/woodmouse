import { parseIsoDay } from "./calendar";

export type ProjectView = "board" | "list" | "calendar" | "timeline";
export type Zoom = "day" | "week" | "month";
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
  /** The day a calendar or timeline is centered on, `YYYY-MM-DD` (default today). */
  date?: string;
  /** The calendar shows a month or a week; the timeline a day, week or month per column. */
  zoom?: Zoom;
}

const oneOf = <T extends string>(value: unknown, allowed: readonly T[]): T | undefined =>
  allowed.find((a) => a === value);
const text = (value: unknown): string | undefined =>
  typeof value === "string" && value ? value : undefined;

/** Drops anything unknown or invalid, so a hand-edited URL can't break the page. */
export function validateProjectSearch(raw: Record<string, unknown>): ProjectSearch {
  return {
    view: oneOf(raw.view, ["board", "list", "calendar", "timeline"]),
    status: oneOf(raw.status, ["open", "done", "all"]),
    assignee: text(raw.assignee),
    label: text(raw.label),
    list: text(raw.list),
    sort: oneOf(raw.sort, ["title", "due_date", "priority"]),
    order: oneOf(raw.order, ["asc", "desc"]),
    date: parseIsoDay(text(raw.date)) !== undefined ? text(raw.date) : undefined,
    zoom: oneOf(raw.zoom, ["day", "week", "month"]),
  };
}

/** What "no status in the URL" means per view: the list shows open tasks, the others show everything. */
export const DEFAULT_STATUS: Record<ProjectView, TaskStatus> = {
  board: "all",
  list: "open",
  calendar: "all",
  timeline: "all",
};

export interface TaskFilterQuery {
  list_id?: string;
  assignee_id?: string;
  label_id?: string;
  is_completed?: boolean;
}

/** The backend filters for the filter params in the URL, so every view reads them the same way. */
export function taskFilterQuery(search: ProjectSearch, view: ProjectView): TaskFilterQuery {
  const status = search.status ?? DEFAULT_STATUS[view];
  return {
    list_id: search.list,
    assignee_id: search.assignee,
    label_id: search.label,
    is_completed: status === "all" ? undefined : status === "done",
  };
}
