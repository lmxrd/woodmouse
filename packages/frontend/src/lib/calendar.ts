/**
 * Day arithmetic for the calendar and timeline. A day is the Unix epoch second of its local midnight
 * (see `time.ts` for converting to and from the API's day values), and all stepping goes through
 * `Date` so daylight-saving changes never shift a day.
 */

import { dayEpoch, dayIso } from "./time";

export type WeekStart = "monday" | "sunday";

const pad = (n: number) => String(n).padStart(2, "0");

/** Local midnight of the day containing `epochSeconds`. */
export function startOfDay(epochSeconds: number): number {
  const d = new Date(epochSeconds * 1000);
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() / 1000);
}

/** The day `days` after (or before, if negative) the given day, at local midnight. */
export function addDays(epochSeconds: number, days: number): number {
  const d = new Date(epochSeconds * 1000);
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate() + days).getTime() / 1000);
}

/** Whole local days from `a` to `b` (negative when `b` is earlier). */
export function daysBetween(a: number, b: number): number {
  const da = new Date(a * 1000);
  const db = new Date(b * 1000);
  const ua = Date.UTC(da.getFullYear(), da.getMonth(), da.getDate());
  const ub = Date.UTC(db.getFullYear(), db.getMonth(), db.getDate());
  return Math.round((ub - ua) / 86_400_000);
}

/** `YYYY-MM-DD` for a day, the form used in URLs. */
export function toIsoDay(epochSeconds: number): string {
  const d = new Date(epochSeconds * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** The day for a `YYYY-MM-DD` string, or undefined when it is not a real date. */
export function parseIsoDay(value: string | undefined): number | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? "");
  if (!m) return undefined;
  const [y, mo, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const d = new Date(y, mo - 1, day);
  if (d.getFullYear() !== y || d.getMonth() !== mo - 1 || d.getDate() !== day) return undefined;
  return Math.floor(d.getTime() / 1000);
}

/** The first day of the week containing `epochSeconds`. */
export function startOfWeek(epochSeconds: number, weekStart: WeekStart): number {
  const day = startOfDay(epochSeconds);
  const weekday = new Date(day * 1000).getDay(); // 0 = Sunday
  const offset = weekStart === "monday" ? (weekday + 6) % 7 : weekday;
  return addDays(day, -offset);
}

/** The seven days of the week containing `epochSeconds`. */
export function weekDays(epochSeconds: number, weekStart: WeekStart): number[] {
  const first = startOfWeek(epochSeconds, weekStart);
  return Array.from({ length: 7 }, (_, i) => addDays(first, i));
}

/** The weeks that cover the month containing `epochSeconds`, each as seven days. */
export function monthGrid(epochSeconds: number, weekStart: WeekStart): number[][] {
  const d = new Date(epochSeconds * 1000);
  const first = Math.floor(new Date(d.getFullYear(), d.getMonth(), 1).getTime() / 1000);
  const last = Math.floor(new Date(d.getFullYear(), d.getMonth() + 1, 0).getTime() / 1000);
  const weeks: number[][] = [];
  for (let day = startOfWeek(first, weekStart); day <= last; day = addDays(day, 7)) {
    weeks.push(Array.from({ length: 7 }, (_, i) => addDays(day, i)));
  }
  return weeks;
}

/** Weekday names in display order, e.g. `["Mon", ..., "Sun"]`. */
export function weekdayLabels(weekStart: WeekStart): string[] {
  const names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  return weekStart === "monday" ? [...names.slice(1), names[0]] : names;
}

export interface Scheduled {
  /** The API's day values (`2026-10-16T00:00:00Z`). */
  start_date?: string | null;
  due_date?: string | null;
}

/**
 * The days a task occupies: from its start date (or due date, if it has none) to its due date (or start
 * date). Null when it has neither.
 */
export function taskSpan(task: Scheduled): { start: number; end: number } | null {
  const a = dayEpoch(task.start_date ?? task.due_date);
  const b = dayEpoch(task.due_date ?? task.start_date);
  if (a == null || b == null) return null;
  return a <= b ? { start: a, end: b } : { start: b, end: a };
}

/** New `start_date`/`due_date` values after moving a task by `days`; dates it lacks stay unset. */
export function shiftDates(
  task: Scheduled,
  days: number,
): { start_date?: string; due_date?: string } {
  const out: { start_date?: string; due_date?: string } = {};
  const start = dayEpoch(task.start_date);
  const due = dayEpoch(task.due_date);
  if (start != null) out.start_date = dayIso(addDays(start, days));
  if (due != null) out.due_date = dayIso(addDays(due, days));
  return out;
}

/**
 * Stack spans into lanes so overlapping ones never share a row: each span gets the first lane whose
 * last span ended before it starts. Returns the lane per span, in input order.
 */
export function assignLanes(spans: { start: number; end: number }[]): number[] {
  const order = spans
    .map((_, i) => i)
    .sort((a, b) => spans[a].start - spans[b].start || spans[b].end - spans[a].end);
  const laneEnds: number[] = [];
  const lanes = Array.from({ length: spans.length }, () => 0);
  for (const i of order) {
    let lane = laneEnds.findIndex((end) => end < spans[i].start);
    if (lane === -1) lane = laneEnds.length;
    laneEnds[lane] = spans[i].end;
    lanes[i] = lane;
  }
  return lanes;
}
