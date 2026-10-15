import { describe, expect, it } from "vitest";

import { dueBucket, fromDateInput, toDateInput } from "./dates";

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime();
const secs = (ms: number) => Math.floor(ms / 1000);

describe("dueBucket", () => {
  const now = at(2026, 10, 15, 14);

  it("puts tasks without a due date in later", () => {
    expect(dueBucket(null, now)).toBe("later");
    expect(dueBucket(undefined, now)).toBe("later");
  });

  it("buckets by local day", () => {
    expect(dueBucket(secs(at(2026, 10, 14, 23)), now)).toBe("overdue");
    expect(dueBucket(secs(at(2026, 10, 15, 0)), now)).toBe("today");
    expect(dueBucket(secs(at(2026, 10, 15, 23)), now)).toBe("today");
    expect(dueBucket(secs(at(2026, 10, 16, 0)), now)).toBe("week");
    expect(dueBucket(secs(at(2026, 10, 21, 12)), now)).toBe("week");
    expect(dueBucket(secs(at(2026, 10, 22, 0)), now)).toBe("later");
  });
});

describe("date input conversion", () => {
  it("round-trips a local date", () => {
    const epoch = fromDateInput("2026-10-18");
    expect(epoch).not.toBeNull();
    expect(toDateInput(epoch)).toBe("2026-10-18");
  });

  it("treats empty input as no date", () => {
    expect(fromDateInput("")).toBeNull();
    expect(toDateInput(null)).toBe("");
  });
});
