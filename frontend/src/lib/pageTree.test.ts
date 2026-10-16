import type { PageSummary } from "woodmouse-client";
import { describe, expect, it } from "vitest";

import { ancestorIds, buildPageTree, subtreeIds } from "./pageTree";

const page = (id: string, parent_id: string | null, position: number): PageSummary => ({
  id,
  parent_id,
  position,
  title: id,
  updated_at: 0,
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
