import type { LinkEdge, List, TaskResponse } from "woodmouse-client";

import {
  addDays,
  daysBetween,
  startOfDay,
  startOfWeek,
  taskSpan,
  type WeekStart,
} from "./calendar";
import type { Zoom } from "./projectSearch";
import { isDone } from "./task";

/** The id of the synthetic list that holds tasks with no list (the project's inbox). */
export const INBOX_ID = "inbox";

export interface Span {
  start: number;
  end: number;
}

/** Pixels per day at each zoom level. */
export const PX_PER_DAY: Record<Zoom, number> = { day: 36, week: 14, month: 4.5 };

export const GROUP_ROW_HEIGHT = 26;
export const TASK_ROW_HEIGHT = 32;

export type TimelineRow =
  | { type: "group"; list: List }
  | {
      type: "task";
      task: TaskResponse;
      depth: number;
      hasChildren: boolean;
      /** The days it occupies: its own dates, or derived from its subtasks when it has none. */
      span: Span | null;
      derived: boolean;
    };

/**
 * The rows of the timeline: each list as a group, its top-level tasks in order, and each task's
 * subtasks nested beneath it unless collapsed. A task without dates of its own takes the span of its
 * subtasks (so a parent covers its children).
 */
export function buildRows(
  lists: List[],
  tasks: TaskResponse[],
  collapsed: ReadonlySet<string>,
): TimelineRow[] {
  // A subtask whose parent is not in `tasks` (filtered out) is shown at the top level instead of vanishing.
  const present = new Set(tasks.map((t) => t.id));
  const isTopLevel = (t: TaskResponse) => !t.parent_id || !present.has(t.parent_id);
  const children = new Map<string, TaskResponse[]>();
  for (const t of tasks) {
    if (!isTopLevel(t)) children.set(t.parent_id!, [...(children.get(t.parent_id!) ?? []), t]);
  }
  for (const list of children.values()) list.sort((a, b) => a.position - b.position);

  // A task's span: its own, or the union of its descendants'.
  const spanOf = (task: TaskResponse): { span: Span | null; derived: boolean } => {
    const own = taskSpan(task);
    if (own) return { span: own, derived: false };
    const spans = (children.get(task.id) ?? [])
      .map((c) => spanOf(c).span)
      .filter((s): s is Span => s !== null);
    if (spans.length === 0) return { span: null, derived: false };
    return {
      span: {
        start: Math.min(...spans.map((s) => s.start)),
        end: Math.max(...spans.map((s) => s.end)),
      },
      derived: true,
    };
  };

  const rows: TimelineRow[] = [];
  const addTask = (task: TaskResponse, depth: number) => {
    const kids = children.get(task.id) ?? [];
    rows.push({ type: "task", task, depth, hasChildren: kids.length > 0, ...spanOf(task) });
    if (!collapsed.has(task.id)) for (const kid of kids) addTask(kid, depth + 1);
  };
  // Tasks in no list form the project's inbox group, first.
  const inbox: List = { id: INBOX_ID, name: "Inbox", project_id: "", position: 0 };
  for (const list of [inbox, ...lists]) {
    const top = tasks
      .filter((t) => (t.list_id ?? INBOX_ID) === list.id && isTopLevel(t))
      .sort((a, b) => a.position - b.position);
    if (top.length === 0) continue;
    rows.push({ type: "group", list });
    for (const task of top) addTask(task, 0);
  }
  return rows;
}

/** The first and last day to draw: around every task and today, padded and at least `minDays` wide. */
export function chartRange(
  spans: Span[],
  today: number,
  weekStart: WeekStart,
  minDays = 60,
): { from: number; to: number } {
  const starts = [today, ...spans.map((s) => s.start)];
  const ends = [today, ...spans.map((s) => s.end)];
  let from = addDays(Math.min(...starts), -7);
  let to = addDays(Math.max(...ends), 28);
  if (daysBetween(from, to) < minDays) to = addDays(from, minDays);
  from = startOfWeek(from, weekStart);
  return { from, to };
}

export interface Tick {
  day: number;
  label: string;
}

/** The axis labels: one per day, per week (its first day), or per month (its first day). */
export function ticks(from: number, to: number, zoom: Zoom, weekStart: WeekStart): Tick[] {
  const out: Tick[] = [];
  const month = (d: number) => new Date(d * 1000).toLocaleString("en", { month: "short" });
  if (zoom === "day") {
    for (let d = from; d <= to; d = addDays(d, 1)) {
      const date = new Date(d * 1000).getDate();
      out.push({ day: d, label: date === 1 ? `${date} ${month(d)}` : String(date) });
    }
  } else if (zoom === "week") {
    for (let d = startOfWeek(from, weekStart); d <= to; d = addDays(d, 7)) {
      out.push({ day: d, label: `${new Date(d * 1000).getDate()} ${month(d)}` });
    }
  } else {
    let year = new Date(from * 1000).getFullYear();
    let monthIndex = new Date(from * 1000).getMonth();
    for (;;) {
      const first = Math.floor(new Date(year, monthIndex, 1).getTime() / 1000);
      if (first > to) break;
      out.push({ day: first, label: `${month(first)} ${String(year).slice(2)}` });
      monthIndex += 1;
      if (monthIndex > 11) [year, monthIndex] = [year + 1, 0];
    }
  }
  return out;
}

/** Left edge and width in pixels for a span, given the chart's first day. */
export function barBox(span: Span, from: number, zoom: Zoom): { x: number; width: number } {
  const px = PX_PER_DAY[zoom];
  return {
    x: daysBetween(from, span.start) * px,
    width: (daysBetween(span.start, span.end) + 1) * px,
  };
}

/** A blocked task should not start before its blocker has ended. */
export function violatesDependency(blocker: Span, blocked: Span): boolean {
  return blocked.start <= blocker.end;
}

export interface Arrow {
  id: string;
  blocker: string;
  blocked: string;
  violated: boolean;
}

/** The dependency arrows to draw: `blocks` links whose two tasks both have a span and a visible row. */
export function dependencyArrows(edges: LinkEdge[], spans: ReadonlyMap<string, Span>): Arrow[] {
  return edges.flatMap((edge) => {
    if (edge.kind !== "blocks") return [];
    const blocker = spans.get(edge.task_id);
    const blocked = spans.get(edge.related_task_id);
    if (!blocker || !blocked) return [];
    return [
      {
        id: edge.id,
        blocker: edge.task_id,
        blocked: edge.related_task_id,
        violated: violatesDependency(blocker, blocked),
      },
    ];
  });
}

/** Progress from 0 to 1: the share of completed subtasks, or all-or-nothing for a task with none. */
export function progressOf(task: TaskResponse): number {
  if (task.subtask_total > 0) return task.subtask_completed / task.subtask_total;
  return isDone(task) ? 1 : 0;
}

export const todayStart = (): number => startOfDay(Math.floor(Date.now() / 1000));

/** Enough days that the chart fills a wide screen at each zoom. */
export const MIN_CHART_DAYS: Record<Zoom, number> = { day: 60, week: 150, month: 420 };
