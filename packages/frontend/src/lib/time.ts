/**
 * The boundary between the API's dates and the app's local-time day arithmetic.
 *
 * The API speaks UTC only, as RFC 3339 with a `Z` (`2026-10-16T08:30:00Z`). Two kinds of value:
 *  - an *instant* (`created_at`, `updated_at`, ...) is a moment; show it in the viewer's local time;
 *  - a *day* (a task's `start_date` and `due_date`) is a calendar date sent as that day at 00:00 UTC. It
 *    means the same date for everyone, so it is never shifted by a timezone.
 *
 * Inside the app, a day is the epoch second of its local midnight (what `calendar.ts` steps through),
 * so the calendar and timeline keep working in local time. These helpers convert at the edges.
 */

const pad = (n: number) => String(n).padStart(2, "0");

/** An instant from the API as epoch seconds. */
export function instant(iso: string): number {
  return Math.floor(new Date(iso).getTime() / 1000);
}

/** A day from the API (`2026-10-16T00:00:00Z`) as the epoch second of that date at local midnight. */
export function dayEpoch(iso: string): number;
export function dayEpoch(iso: string | null | undefined): number | null;
export function dayEpoch(iso: string | null | undefined): number | null {
  if (!iso) return null;
  const [y, m, d] = iso.slice(0, 10).split("-").map(Number);
  return Math.floor(new Date(y, m - 1, d).getTime() / 1000);
}

/** A local day (any epoch second within it) as the API's day value: that date at 00:00 UTC. */
export function dayIso(epochSeconds: number): string {
  const d = new Date(epochSeconds * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T00:00:00Z`;
}
