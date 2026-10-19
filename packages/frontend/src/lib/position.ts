/**
 * Where a dropped item goes, as the API wants it: the id of the sibling it now follows, or `null` for
 * the first place. `siblings` are in order and do not include the moved item; `index` is the slot it was
 * dropped into. The server picks the number, so a drop only ever changes the moved item.
 */
export function afterIdAt(siblings: { id: string }[], index: number): string | null {
  return siblings[index - 1]?.id ?? null;
}

/**
 * A provisional position for an item placed right after `afterId` (`null`: first), to show the move
 * before the server answers. `siblings` are sorted by position and do not include the moved item. The
 * server's own numbers replace this on the next refresh.
 */
export function provisionalPosition(
  siblings: { id: string; position: number }[],
  afterId: string | null,
): number {
  const index = afterId == null ? 0 : siblings.findIndex((s) => s.id === afterId) + 1;
  const before = index > 0 ? siblings[index - 1] : undefined;
  const next = siblings[index];
  if (before && next) return (before.position + next.position) / 2;
  if (before) return before.position + 1;
  if (next) return next.position - 1;
  return 0;
}
