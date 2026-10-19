/** Display preferences from the user's account (see PublicUser). */
export interface DisplayPrefs {
  date_format: string;
  time_format: string;
  /** IANA timezone for showing instants; the browser's own when absent or unknown. */
  timezone?: string | null;
}

interface Parts {
  year: number;
  month: number; // 0-11
  day: number;
  hour: number;
  minute: number;
}

/** The wall-clock parts of an instant in `timezone`, or in the browser's timezone when that is unknown. */
function partsOf(epochSeconds: number, timezone?: string | null): Parts {
  const d = new Date(epochSeconds * 1000);
  if (timezone) {
    try {
      const f = new Intl.DateTimeFormat("en-GB", {
        timeZone: timezone,
        hourCycle: "h23",
        year: "numeric",
        month: "numeric",
        day: "numeric",
        hour: "numeric",
        minute: "numeric",
      });
      const v = Object.fromEntries(f.formatToParts(d).map((p) => [p.type, Number(p.value)]));
      return { year: v.year, month: v.month - 1, day: v.day, hour: v.hour, minute: v.minute };
    } catch {
      // an unknown timezone name: fall through to the browser's
    }
  }
  return {
    year: d.getFullYear(),
    month: d.getMonth(),
    day: d.getDate(),
    hour: d.getHours(),
    minute: d.getMinutes(),
  };
}

export const DEFAULT_PREFS: DisplayPrefs = { date_format: "YYYY-MM-DD", time_format: "24h" };

export const DATE_FORMATS = ["YYYY-MM-DD", "DD.MM.YYYY", "MM/DD/YYYY", "D MMM YYYY"] as const;
export const TIME_FORMATS = ["24h", "12h"] as const;

/** Anything newer than this reads as "2 h ago"; older shows the absolute date and time. */
const RELATIVE_WINDOW_SECONDS = 7 * 24 * 60 * 60;

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (n: number) => String(n).padStart(2, "0");

/** The full date, e.g. `2026-10-18`, `18.10.2026`, `10/18/2026` or `18 Oct 2026`. */
export function formatDate(epochSeconds: number, prefs: DisplayPrefs = DEFAULT_PREFS): string {
  const p = partsOf(epochSeconds, prefs.timezone);
  const [y, m, day] = [p.year, p.month, p.day];
  switch (prefs.date_format) {
    case "DD.MM.YYYY":
      return `${pad(day)}.${pad(m + 1)}.${y}`;
    case "MM/DD/YYYY":
      return `${pad(m + 1)}/${pad(day)}/${y}`;
    case "D MMM YYYY":
      return `${day} ${MONTHS[m]} ${y}`;
    default:
      return `${y}-${pad(m + 1)}-${pad(day)}`;
  }
}

/** A short date for tight spaces such as board cards: no year when it is the current year. */
export function formatDateShort(
  epochSeconds: number,
  prefs: DisplayPrefs = DEFAULT_PREFS,
  now = Date.now(),
): string {
  const p = partsOf(epochSeconds, prefs.timezone);
  if (p.year !== partsOf(now / 1000, prefs.timezone).year) return formatDate(epochSeconds, prefs);
  const [m, day] = [p.month, p.day];
  switch (prefs.date_format) {
    case "DD.MM.YYYY":
      return `${pad(day)}.${pad(m + 1)}.`;
    case "MM/DD/YYYY":
      return `${pad(m + 1)}/${pad(day)}`;
    case "D MMM YYYY":
      return `${day} ${MONTHS[m]}`;
    default:
      return `${pad(m + 1)}-${pad(day)}`;
  }
}

/** The time of day: `14:30` on a 24 hour clock, `2:30 PM` on a 12 hour clock. */
export function formatTime(epochSeconds: number, prefs: DisplayPrefs = DEFAULT_PREFS): string {
  const p = partsOf(epochSeconds, prefs.timezone);
  if (prefs.time_format === "12h") {
    const hour = p.hour % 12 || 12;
    return `${hour}:${pad(p.minute)} ${p.hour < 12 ? "AM" : "PM"}`;
  }
  return `${pad(p.hour)}:${pad(p.minute)}`;
}

export function formatDateTime(epochSeconds: number, prefs: DisplayPrefs = DEFAULT_PREFS): string {
  return `${formatDate(epochSeconds, prefs)} ${formatTime(epochSeconds, prefs)}`;
}

/**
 * "just now", "5 min ago", "2 h ago", "3 days ago" for the last seven days; the absolute date and
 * time (in the user's format) for anything older.
 */
export function formatWhen(
  epochSeconds: number,
  prefs: DisplayPrefs = DEFAULT_PREFS,
  now = Date.now(),
): string {
  const age = Math.floor(now / 1000) - epochSeconds;
  if (age > RELATIVE_WINDOW_SECONDS) return formatDateTime(epochSeconds, prefs);
  if (age < 60) return "just now"; // also covers small clock differences into the future
  if (age < 3600) return `${Math.floor(age / 60)} min ago`;
  if (age < 86400) return `${Math.floor(age / 3600)} h ago`;
  const days = Math.floor(age / 86400);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/** The calendar day of an instant in the person's timezone, as `YYYY-MM-DD` (for grouping by day). */
export function dayKey(epochSeconds: number, prefs: DisplayPrefs = DEFAULT_PREFS): string {
  const p = partsOf(epochSeconds, prefs.timezone);
  return `${p.year}-${pad(p.month + 1)}-${pad(p.day)}`;
}
