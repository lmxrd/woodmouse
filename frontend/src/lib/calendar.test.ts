import { describe, expect, it } from "vitest";

import {
  addDays,
  assignLanes,
  daysBetween,
  monthGrid,
  parseIsoDay,
  shiftDates,
  startOfWeek,
  taskSpan,
  toIsoDay,
  weekdayLabels,
} from "./calendar";

const day = (y: number, m: number, d: number) => Math.floor(new Date(y, m - 1, d).getTime() / 1000);

describe("day arithmetic", () => {
  it("steps whole local days, also across month ends", () => {
    expect(toIsoDay(addDays(day(2026, 10, 31), 1))).toBe("2026-11-01");
    expect(toIsoDay(addDays(day(2026, 3, 1), -1))).toBe("2026-02-28");
  });
  it("counts days between, signed", () => {
    expect(daysBetween(day(2026, 10, 15), day(2026, 10, 18))).toBe(3);
    expect(daysBetween(day(2026, 10, 18), day(2026, 10, 15))).toBe(-3);
    expect(daysBetween(day(2026, 10, 15), day(2026, 10, 15))).toBe(0);
  });
  it("stays on whole days across a daylight-saving change", () => {
    // Late March / late October are DST changes in many zones; the count must still be exact.
    expect(daysBetween(day(2026, 3, 20), day(2026, 4, 5))).toBe(16);
    expect(daysBetween(day(2026, 10, 20), day(2026, 11, 5))).toBe(16);
    expect(toIsoDay(addDays(day(2026, 3, 28), 2))).toBe("2026-03-30");
  });
  it("round-trips ISO days and rejects impossible ones", () => {
    expect(toIsoDay(parseIsoDay("2026-10-05")!)).toBe("2026-10-05");
    expect(parseIsoDay("2026-02-30")).toBeUndefined();
    expect(parseIsoDay("nope")).toBeUndefined();
    expect(parseIsoDay(undefined)).toBeUndefined();
  });
});

describe("weeks and months", () => {
  it("starts the week on Monday or Sunday", () => {
    const thursday = day(2026, 10, 15);
    expect(toIsoDay(startOfWeek(thursday, "monday"))).toBe("2026-10-12");
    expect(toIsoDay(startOfWeek(thursday, "sunday"))).toBe("2026-10-11");
    expect(toIsoDay(startOfWeek(day(2026, 10, 18), "monday"))).toBe("2026-10-12"); // a Sunday
    expect(toIsoDay(startOfWeek(day(2026, 10, 18), "sunday"))).toBe("2026-10-18");
  });
  it("covers the month with whole weeks", () => {
    const weeks = monthGrid(day(2026, 10, 15), "monday");
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    expect(toIsoDay(weeks[0][0])).toBe("2026-09-28");
    expect(toIsoDay(weeks[weeks.length - 1][6])).toBe("2026-11-01");
    expect(weeks).toHaveLength(5);
    // February 2026 starts on a Sunday: with Monday weeks it needs a trailing partial week.
    expect(monthGrid(day(2026, 2, 10), "monday")).toHaveLength(5);
    expect(monthGrid(day(2026, 2, 10), "sunday")).toHaveLength(4);
  });
  it("labels weekdays in display order", () => {
    expect(weekdayLabels("monday")[0]).toBe("Mon");
    expect(weekdayLabels("monday")[6]).toBe("Sun");
    expect(weekdayLabels("sunday")[0]).toBe("Sun");
  });
});

describe("task scheduling", () => {
  it("spans from start to due, or a single day", () => {
    const [a, b] = [day(2026, 10, 12), day(2026, 10, 15)];
    expect(taskSpan({ start_date: a, due_date: b })).toEqual({ start: a, end: b });
    expect(taskSpan({ due_date: b })).toEqual({ start: b, end: b });
    expect(taskSpan({ start_date: a })).toEqual({ start: a, end: a });
    expect(taskSpan({})).toBeNull();
  });
  it("tolerates a start after the due date", () => {
    const [a, b] = [day(2026, 10, 15), day(2026, 10, 12)];
    expect(taskSpan({ start_date: a, due_date: b })).toEqual({ start: b, end: a });
  });
  it("moves both dates by the same amount and leaves missing ones unset", () => {
    const [a, b] = [day(2026, 10, 12), day(2026, 10, 15)];
    const moved = shiftDates({ start_date: a, due_date: b }, 3);
    expect(toIsoDay(moved.start_date!)).toBe("2026-10-15");
    expect(toIsoDay(moved.due_date!)).toBe("2026-10-18");
    expect(shiftDates({ due_date: b }, -1)).toEqual({ due_date: addDays(b, -1) });
  });
});

describe("assignLanes", () => {
  it("puts overlapping spans in different lanes and reuses freed lanes", () => {
    const spans = [
      { start: 1, end: 5 },
      { start: 2, end: 3 },
      { start: 4, end: 6 },
      { start: 6, end: 7 },
    ];
    expect(assignLanes(spans)).toEqual([0, 1, 1, 0]);
  });
  it("needs a single lane for spans that never overlap", () => {
    expect(
      assignLanes([
        { start: 1, end: 1 },
        { start: 2, end: 2 },
      ]),
    ).toEqual([0, 0]);
  });
});
