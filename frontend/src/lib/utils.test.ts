import { describe, expect, it } from "vitest";

import { cn } from "./utils";

describe("cn", () => {
  it("joins class names and skips falsy values", () => {
    expect(cn("a", "b")).toBe("a b");
    expect(cn("a", false, undefined, null, "c")).toBe("a c");
    expect(cn()).toBe("");
  });
});
