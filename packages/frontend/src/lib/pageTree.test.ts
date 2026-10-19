import type { PageSummary } from "woodmouse-sdk";
import { describe, expect, it } from "vitest";

import { ancestorIds, buildPageTree, dropChange, subtreeIds, zoneAt } from "./pageTree";

const page = (id: string, parent_id: string | null, position: number): PageSummary => ({
  id,
  parent_id,
  position,
  title: id,
  updated_at: "2026-10-16T00:00:00Z",
});
const pages = [page("c", "a", 2), page("b", null, 2), page("a", null, 1), page("d", "c", 1)];

describe("page tree", () => {
  it("nests by parent and orders by position", () => {
    const tree = buildPageTree(pages);
    expect(tree.map((n) => n.page.id)).toEqual(["a", "b"]);
    expect(tree[0].children[0].children[0]).toMatchObject({ page: { id: "d" }, depth: 2 });
  });

  it("finds subtrees and ancestors", () => {
    expect([...subtreeIds(pages, "a")].sort()).toEqual(["a", "c", "d"]);
    expect(ancestorIds(pages, "d")).toEqual(["c", "a"]);
  });

  it("shows orphans at the top", () => {
    expect(buildPageTree([page("x", "gone", 1)]).map((n) => n.page.id)).toEqual(["x"]);
  });
});

describe("dropping a page", () => {
  // a(1) b(2) at the top; c(2) and its child d below a.
  it("puts it before or after a sibling by naming the sibling it follows", () => {
    expect(dropChange(pages, "b", "a", "before")).toEqual({ parent_id: null, after_id: null });
    expect(dropChange(pages, "a", "b", "after")).toEqual({ parent_id: null, after_id: "b" });
    expect(dropChange(pages, "d", "c", "before")).toEqual({ parent_id: "a", after_id: null });
  });

  it("nests into the target as its last child", () => {
    expect(dropChange(pages, "b", "a", "into")).toEqual({ parent_id: "a" });
  });

  it("refuses to drop a page onto itself or below itself", () => {
    expect(dropChange(pages, "a", "a", "into")).toBeNull();
    expect(dropChange(pages, "a", "d", "before")).toBeNull();
  });

  it("tells the zone from the pointer position", () => {
    expect(zoneAt(101, 100, 40)).toBe("before");
    expect(zoneAt(120, 100, 40)).toBe("into");
    expect(zoneAt(139, 100, 40)).toBe("after");
  });
});
