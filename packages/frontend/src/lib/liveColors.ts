/** Colours for other people's cursors; a person always gets the same one. */
const COLORS = [
  "#1f4d3a",
  "#b4540b",
  "#6b3fa0",
  "#0f6b8a",
  "#a1304a",
  "#5b6b1f",
  "#8a5a00",
  "#3b4f9a",
];

export function colorFor(id: string): string {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return COLORS[hash % COLORS.length];
}
