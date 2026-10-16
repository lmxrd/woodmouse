import type { PageSummary } from "woodmouse-client";

import { positionBetween } from "./position";

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

export type DropZone = "before" | "into" | "after";

/** Which part of a row the pointer is over: the top quarter, the bottom quarter, or the middle. */
export function zoneAt(pointerY: number, top: number, height: number): DropZone {
  const ratio = (pointerY - top) / height;
  return ratio < 0.25 ? "before" : ratio > 0.75 ? "after" : "into";
}

/**
 * What dropping page `dragged` on `target` changes: `before` and `after` make it a sibling at that spot,
 * `into` makes it the last child. `null` when nothing should happen (onto itself or below itself).
 */
export function dropChange(
  pages: PageSummary[],
  dragged: string,
  target: string,
  zone: DropZone,
): { parent_id: string | null; position?: number } | null {
  if (subtreeIds(pages, dragged).has(target)) return null;
  const to = pages.find((p) => p.id === target);
  if (!to) return null;
  if (zone === "into") return { parent_id: target };
  const parent = to.parent_id ?? null;
  const siblings = pages
    .filter((p) => (p.parent_id ?? null) === parent && p.id !== dragged)
    .sort((a, b) => a.position - b.position);
  const index = siblings.findIndex((p) => p.id === target) + (zone === "after" ? 1 : 0);
  return { parent_id: parent, position: positionBetween(siblings, index) };
}
