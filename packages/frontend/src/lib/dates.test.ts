import { describe, expect, it } from "vitest";

import { dueBucket, fromDateInput, toDateInput } from "./dates";

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime();
/** The API's day value for a date: that day at 00:00 UTC. */
const dayValue = (y: number, m: number, d: number) =>
  `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}T00:00:00Z`;

describe("dueBucket", () => {
  const now = at(2026, 10, 15, 14);

  it("puts tasks without a due date in later", () => {
    expect(dueBucket(null, now)).toBe("later");
    expect(dueBucket(undefined, now)).toBe("later");
  });

  it("buckets by local day", () => {
    expect(dueBucket(dayValue(2026, 10, 14), now)).toBe("overdue");
    expect(dueBucket(dayValue(2026, 10, 15), now)).toBe("today");
    expect(dueBucket(dayValue(2026, 10, 15), now)).toBe("today");
    expect(dueBucket(dayValue(2026, 10, 16), now)).toBe("week");
    expect(dueBucket(dayValue(2026, 10, 21), now)).toBe("week");
    expect(dueBucket(dayValue(2026, 10, 22), now)).toBe("later");
  });
});

describe("date input conversion", () => {
  it("sends a date as that day at 00:00 UTC, whatever the local timezone", () => {
    expect(fromDateInput("2026-10-18")).toBe("2026-10-18T00:00:00Z");
    expect(toDateInput("2026-10-18T00:00:00Z")).toBe("2026-10-18");
  });

  it("treats empty input as no date", () => {
    expect(fromDateInput("")).toBeNull();
    expect(toDateInput(null)).toBe("");
  });
});
