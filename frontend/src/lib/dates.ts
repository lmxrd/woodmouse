/** True when the date is today or earlier (and so worth the signal color). */
export function isDueNow(epochSeconds: number, now = Date.now()): boolean {
  const endOfToday = new Date(now);
  endOfToday.setHours(23, 59, 59, 999);
  return epochSeconds * 1000 <= endOfToday.getTime();
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Unix epoch seconds to the `YYYY-MM-DD` string an `<input type="date">` uses (local time). */
export function toDateInput(epochSeconds: number | null | undefined): string {
  if (epochSeconds == null) return "";
  const d = new Date(epochSeconds * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** `YYYY-MM-DD` to epoch seconds at local midnight; empty string means no date. */
export function fromDateInput(value: string): number | null {
  if (!value) return null;
  const [year, month, day] = value.split("-").map(Number);
  return Math.floor(new Date(year, month - 1, day).getTime() / 1000);
}

export type DueBucket = "overdue" | "today" | "week" | "later";

/**
 * Where a due date falls relative to today (local time): before today, today, the next six days, or
 * later/none. Used to group the "My tasks" page.
 */
export function dueBucket(epochSeconds: number | null | undefined, now = Date.now()): DueBucket {
  if (epochSeconds == null) return "later";
  const startOfToday = new Date(now);
  startOfToday.setHours(0, 0, 0, 0);
  const tomorrow = new Date(startOfToday);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const weekEnd = new Date(startOfToday);
  weekEnd.setDate(weekEnd.getDate() + 7);

  const due = epochSeconds * 1000;
  if (due < startOfToday.getTime()) return "overdue";
  if (due < tomorrow.getTime()) return "today";
  if (due < weekEnd.getTime()) return "week";
  return "later";
}
