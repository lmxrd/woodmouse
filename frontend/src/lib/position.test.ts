import { describe, expect, it } from "vitest";

import { positionBetween } from "./position";

const col = (...positions: number[]) => positions.map((position) => ({ position }));

describe("positionBetween", () => {
  it("lands in the middle between two neighbours", () => {
    expect(positionBetween(col(1, 2, 3), 1)).toBe(1.5);
    expect(positionBetween(col(1, 2, 3), 2)).toBe(2.5);
  });
  it("goes before the first and after the last", () => {
    expect(positionBetween(col(1, 2), 0)).toBe(0);
    expect(positionBetween(col(1, 2), 2)).toBe(3);
  });
  it("starts an empty column at 1", () => {
    expect(positionBetween([], 0)).toBe(1);
  });
  it("keeps splitting the same gap without collisions for a long while", () => {
    let siblings = col(1, 2);
    for (let i = 0; i < 40; i++) {
      const p = positionBetween(siblings, 1);
      expect(p).toBeGreaterThan(siblings[0].position);
      expect(p).toBeLessThan(siblings[1].position);
      siblings = [siblings[0], { position: p }, siblings[1]].filter((_, i2) => i2 !== 2);
    }
  });
});
