/** Display preferences from the user's account (see PublicUser). */
export interface DisplayPrefs {
  date_format: string;
  time_format: string;
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
  const d = new Date(epochSeconds * 1000);
  const [y, m, day] = [d.getFullYear(), d.getMonth(), d.getDate()];
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
  const d = new Date(epochSeconds * 1000);
  if (d.getFullYear() !== new Date(now).getFullYear()) return formatDate(epochSeconds, prefs);
  const [m, day] = [d.getMonth(), d.getDate()];
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
  const d = new Date(epochSeconds * 1000);
  if (prefs.time_format === "12h") {
    const hour = d.getHours() % 12 || 12;
    return `${hour}:${pad(d.getMinutes())} ${d.getHours() < 12 ? "AM" : "PM"}`;
  }
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
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
