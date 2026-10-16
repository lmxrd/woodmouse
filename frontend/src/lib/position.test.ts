import { describe, expect, it } from "vitest";

import { afterIdAt, provisionalPosition } from "./position";

const items = [
  { id: "a", position: 20 },
  { id: "b", position: 40 },
  { id: "c", position: 60 },
];

describe("afterIdAt", () => {
  it("names the sibling a drop follows, or null for the first place", () => {
    expect(afterIdAt(items, 0)).toBeNull();
    expect(afterIdAt(items, 1)).toBe("a");
    expect(afterIdAt(items, 3)).toBe("c");
    expect(afterIdAt([], 0)).toBeNull();
  });
});

describe("provisionalPosition", () => {
  it("goes between the neighbours, or past either end", () => {
    expect(provisionalPosition(items, "a")).toBe(30);
    expect(provisionalPosition(items, null)).toBe(19);
    expect(provisionalPosition(items, "c")).toBe(61);
    expect(provisionalPosition([], null)).toBe(0);
  });
});
