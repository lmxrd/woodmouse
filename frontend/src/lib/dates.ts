const dayFormat = new Intl.DateTimeFormat("en", { month: "short", day: "numeric" });

/** "Oct 18" from Unix epoch seconds. */
export function formatDay(epochSeconds: number): string {
  return dayFormat.format(new Date(epochSeconds * 1000));
}

/** True when the date is today or earlier (and so worth the signal color). */
export function isDueNow(epochSeconds: number, now = Date.now()): boolean {
  const endOfToday = new Date(now);
  endOfToday.setHours(23, 59, 59, 999);
  return epochSeconds * 1000 <= endOfToday.getTime();
}
