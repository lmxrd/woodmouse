/**
 * The ordering position for a task dropped at `index` among `siblings` (already sorted by position,
 * and not including the task itself): midway between its new neighbours, or one step past the end.
 * Positions are floats, so a drop only ever changes the moved task.
 */
export function positionBetween(siblings: { position: number }[], index: number): number {
  const before = siblings[index - 1];
  const after = siblings[index];
  if (before && after) return (before.position + after.position) / 2;
  if (before) return before.position + 1;
  if (after) return after.position - 1;
  return 1;
}
