/**
 * Work time is stored as whole minutes. People type it the way they say it: `90`, `45m`, `2h`, `1h 30m`,
 * `1.5h` or `1:30` (a bare number is minutes).
 */

/** Minutes for what was typed; `null` for empty (no value), `undefined` when it cannot be read. */
export function parseDuration(text: string): number | null | undefined {
  const t = text.trim().toLowerCase();
  if (!t) return null;
  const clock = /^(\d+):([0-5]?\d)$/.exec(t);
  if (clock) return Number(clock[1]) * 60 + Number(clock[2]);
  if (/^\d+(\.\d+)?$/.test(t)) return Math.round(Number(t));
  const m = /^(?:(\d+(?:\.\d+)?)\s*h(?:ours?|rs?)?)?\s*(?:(\d+)\s*m(?:in(?:ute)?s?)?)?$/.exec(t);
  if (!m || (m[1] === undefined && m[2] === undefined)) return undefined;
  return Math.round(Number(m[1] ?? 0) * 60 + Number(m[2] ?? 0));
}

/** `1h 30m`, `45m`, `2h`; empty for no value. */
export function formatDuration(minutes: number | null | undefined): string {
  if (minutes == null) return "";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}m`;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}
