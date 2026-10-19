import { describe, expect, it } from "vitest";

import {
  formatDate,
  formatDateShort,
  formatDateTime,
  formatTime,
  formatWhen,
  type DisplayPrefs,
} from "./format";

const at = (y: number, m: number, d: number, h = 12, min = 0) =>
  Math.floor(new Date(y, m - 1, d, h, min).getTime() / 1000);
const prefs = (date_format: string, time_format = "24h"): DisplayPrefs => ({
  date_format,
  time_format,
});

describe("formatDate", () => {
  const epoch = at(2026, 10, 5);
  it("supports each format", () => {
    expect(formatDate(epoch, prefs("YYYY-MM-DD"))).toBe("2026-10-05");
    expect(formatDate(epoch, prefs("DD.MM.YYYY"))).toBe("05.10.2026");
    expect(formatDate(epoch, prefs("MM/DD/YYYY"))).toBe("10/05/2026");
    expect(formatDate(epoch, prefs("D MMM YYYY"))).toBe("5 Oct 2026");
  });
  it("defaults to ISO for unknown values", () => {
    expect(formatDate(epoch, prefs("whatever"))).toBe("2026-10-05");
  });
});

describe("formatDateShort", () => {
  const now = new Date(2026, 9, 15).getTime();
  it("drops the year within the current year", () => {
    expect(formatDateShort(at(2026, 10, 18), prefs("YYYY-MM-DD"), now)).toBe("10-18");
    expect(formatDateShort(at(2026, 10, 18), prefs("DD.MM.YYYY"), now)).toBe("18.10.");
    expect(formatDateShort(at(2026, 10, 18), prefs("MM/DD/YYYY"), now)).toBe("10/18");
    expect(formatDateShort(at(2026, 10, 18), prefs("D MMM YYYY"), now)).toBe("18 Oct");
  });
  it("keeps the year for other years", () => {
    expect(formatDateShort(at(2025, 10, 18), prefs("YYYY-MM-DD"), now)).toBe("2025-10-18");
  });
});

describe("formatTime", () => {
  it("handles 24 and 12 hour clocks", () => {
    expect(formatTime(at(2026, 10, 5, 14, 30), prefs("YYYY-MM-DD", "24h"))).toBe("14:30");
    expect(formatTime(at(2026, 10, 5, 14, 30), prefs("YYYY-MM-DD", "12h"))).toBe("2:30 PM");
    expect(formatTime(at(2026, 10, 5, 0, 5), prefs("YYYY-MM-DD", "12h"))).toBe("12:05 AM");
    expect(formatTime(at(2026, 10, 5, 12, 0), prefs("YYYY-MM-DD", "12h"))).toBe("12:00 PM");
    expect(formatTime(at(2026, 10, 5, 0, 5), prefs("YYYY-MM-DD", "24h"))).toBe("00:05");
  });
  it("joins date and time", () => {
    expect(formatDateTime(at(2026, 10, 2, 14, 30))).toBe("2026-10-02 14:30");
  });
});

describe("formatWhen", () => {
  const now = new Date(2026, 9, 15, 12, 0).getTime();
  const ago = (seconds: number) => Math.floor(now / 1000) - seconds;

  it("is relative for the first seven days", () => {
    expect(formatWhen(ago(10), undefined, now)).toBe("just now");
    expect(formatWhen(ago(5 * 60), undefined, now)).toBe("5 min ago");
    expect(formatWhen(ago(2 * 3600), undefined, now)).toBe("2 h ago");
    expect(formatWhen(ago(24 * 3600), undefined, now)).toBe("1 day ago");
    expect(formatWhen(ago(5 * 86400), undefined, now)).toBe("5 days ago");
    expect(formatWhen(ago(7 * 86400), undefined, now)).toBe("7 days ago");
  });

  it("switches to the absolute date and time after seven days, in the user's format", () => {
    const old = ago(7 * 86400 + 60);
    expect(formatWhen(old, prefs("YYYY-MM-DD"), now)).toMatch(/^2026-10-0\d \d\d:\d\d$/);
    expect(formatWhen(at(2026, 10, 2, 14, 30), prefs("DD.MM.YYYY", "12h"), now)).toBe(
      "02.10.2026 2:30 PM",
    );
  });

  it("treats timestamps slightly in the future as just now", () => {
    expect(formatWhen(ago(-30), undefined, now)).toBe("just now");
  });
});

describe("timezones", () => {
  // 2026-10-16 23:30 UTC is already the 17th in Oslo (UTC+2 in October) and still the 16th in New York.
  const epoch = Date.UTC(2026, 9, 16, 23, 30) / 1000;
  it("shows an instant in the person's timezone", () => {
    const oslo: DisplayPrefs = {
      date_format: "YYYY-MM-DD",
      time_format: "24h",
      timezone: "Europe/Oslo",
    };
    const ny: DisplayPrefs = { ...oslo, timezone: "America/New_York" };
    expect(formatDateTime(epoch, oslo)).toBe("2026-10-17 01:30");
    expect(formatDateTime(epoch, ny)).toBe("2026-10-16 19:30");
  });
  it("falls back to the browser's timezone when the name is unknown", () => {
    const bad: DisplayPrefs = {
      date_format: "YYYY-MM-DD",
      time_format: "24h",
      timezone: "Nope/Nowhere",
    };
    expect(formatDateTime(epoch, bad)).toBe(formatDateTime(epoch, prefs("YYYY-MM-DD")));
  });
});
