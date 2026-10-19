import { describe, expect, it } from "vitest";

import { dayEpoch, dayIso, instant } from "./time";

describe("days", () => {
  it("round-trip between the API's day value and local midnight", () => {
    const epoch = dayEpoch("2026-10-16T00:00:00Z");
    const d = new Date(epoch * 1000);
    expect([d.getFullYear(), d.getMonth() + 1, d.getDate(), d.getHours()]).toEqual([
      2026, 10, 16, 0,
    ]);
    expect(dayIso(epoch)).toBe("2026-10-16T00:00:00Z");
  });

  it("treats a missing day as none", () => {
    expect(dayEpoch(null)).toBeNull();
    expect(dayEpoch(undefined)).toBeNull();
  });
});

describe("instants", () => {
  it("are read as UTC", () => {
    expect(instant("2026-10-16T08:30:00Z")).toBe(Date.UTC(2026, 9, 16, 8, 30) / 1000);
  });
});
