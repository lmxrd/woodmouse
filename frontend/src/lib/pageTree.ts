import type { PageSummary } from "woodmouse-client";

export interface PageNode {
  page: PageSummary;
  depth: number;
  children: PageNode[];
}

/** Build the page tree from the flat list. Pages whose parent is missing are shown at the top. */
export function buildPageTree(pages: PageSummary[]): PageNode[] {
  const ids = new Set(pages.map((p) => p.id));
  const byParent = new Map<string | null, PageSummary[]>();
  for (const page of pages) {
    const parent = page.parent_id && ids.has(page.parent_id) ? page.parent_id : null;
    byParent.set(parent, [...(byParent.get(parent) ?? []), page]);
  }
  const build = (parent: string | null, depth: number): PageNode[] =>
    (byParent.get(parent) ?? [])
      .sort((a, b) => a.position - b.position)
      .map((page) => ({ page, depth, children: build(page.id, depth + 1) }));
  return build(null, 0);
}

/** The page and everything below it. */
export function subtreeIds(pages: PageSummary[], id: string): Set<string> {
  const result = new Set([id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const p of pages) {
      if (p.parent_id && result.has(p.parent_id) && !result.has(p.id)) {
        result.add(p.id);
        grew = true;
      }
    }
  }
  return result;
}

/** Ids of the pages above `id`, nearest first. */
export function ancestorIds(pages: PageSummary[], id: string): string[] {
  const byId = new Map(pages.map((p) => [p.id, p]));
  const result: string[] = [];
  let cursor = byId.get(id)?.parent_id;
  while (cursor && result.length < 64) {
    result.push(cursor);
    cursor = byId.get(cursor)?.parent_id;
  }
  return result;
}
