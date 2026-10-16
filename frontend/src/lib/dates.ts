import { dayEpoch, dayIso } from "./time";

/** True when the due day (an API day value) is today or earlier, and so worth the signal color. */
export function isDueNow(due: string, now = Date.now()): boolean {
  const endOfToday = new Date(now);
  endOfToday.setHours(23, 59, 59, 999);
  return dayEpoch(due) * 1000 <= endOfToday.getTime();
}

/** An API day value to the `YYYY-MM-DD` string an `<input type="date">` uses. */
export function toDateInput(day: string | null | undefined): string {
  return day ? day.slice(0, 10) : "";
}

/** `YYYY-MM-DD` to the API's day value (that date at 00:00 UTC); empty means no date. */
export function fromDateInput(value: string): string | null {
  if (!value) return null;
  const [year, month, d] = value.split("-").map(Number);
  return dayIso(Math.floor(new Date(year, month - 1, d).getTime() / 1000));
}

export type DueBucket = "overdue" | "today" | "week" | "later";

/**
 * Where a due day falls relative to today (local time): before today, today, the next six days, or
 * later/none. Used to group the "My tasks" page.
 */
export function dueBucket(due: string | null | undefined, now = Date.now()): DueBucket {
  const dueEpoch = dayEpoch(due);
  if (dueEpoch == null) return "later";
  const startOfToday = new Date(now);
  startOfToday.setHours(0, 0, 0, 0);
  const tomorrow = new Date(startOfToday);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const weekEnd = new Date(startOfToday);
  weekEnd.setDate(weekEnd.getDate() + 7);

  const at = dueEpoch * 1000;
  if (at < startOfToday.getTime()) return "overdue";
  if (at < tomorrow.getTime()) return "today";
  if (at < weekEnd.getTime()) return "week";
  return "later";
}
