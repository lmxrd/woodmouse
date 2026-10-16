import { describe, expect, it } from "vitest";

import { formatDuration, parseDuration } from "./duration";

describe("parseDuration", () => {
  it("reads the ways people write work time", () => {
    expect(parseDuration("90")).toBe(90);
    expect(parseDuration("45m")).toBe(45);
    expect(parseDuration("2h")).toBe(120);
    expect(parseDuration("1h 30m")).toBe(90);
    expect(parseDuration("1h30")).toBeUndefined();
    expect(parseDuration("1.5h")).toBe(90);
    expect(parseDuration("1:30")).toBe(90);
    expect(parseDuration("  2 hours 5 min ")).toBe(125);
  });
  it("treats empty as no value and nonsense as invalid", () => {
    expect(parseDuration("")).toBeNull();
    expect(parseDuration("   ")).toBeNull();
    expect(parseDuration("soon")).toBeUndefined();
    expect(parseDuration("-5")).toBeUndefined();
  });
});

describe("formatDuration", () => {
  it("writes minutes compactly", () => {
    expect(formatDuration(0)).toBe("0m");
    expect(formatDuration(45)).toBe("45m");
    expect(formatDuration(120)).toBe("2h");
    expect(formatDuration(90)).toBe("1h 30m");
    expect(formatDuration(null)).toBe("");
  });
});
