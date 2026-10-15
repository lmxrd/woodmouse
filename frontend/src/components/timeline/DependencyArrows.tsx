import type { Arrow } from "../../lib/timeline";

export interface Anchor {
  /** Left and right edge of the task's bar, in chart pixels. */
  left: number;
  right: number;
  /** Vertical middle of its row. */
  y: number;
}

/**
 * Elbow arrows from the end of a blocker's bar to the start of the task it blocks. Arrows turn red
 * when the blocked task starts before its blocker has ended. Purely advisory: nothing is enforced.
 */
export function DependencyArrows({
  arrows,
  anchors,
  width,
  height,
}: {
  arrows: Arrow[];
  anchors: ReadonlyMap<string, Anchor>;
  width: number;
  height: number;
}) {
  return (
    <svg
      aria-hidden
      className="pointer-events-none absolute top-0 left-0"
      width={width}
      height={height}
    >
      <defs>
        {(["ok", "late"] as const).map((kind) => (
          <marker
            key={kind}
            id={`arrow-${kind}`}
            viewBox="0 0 8 8"
            refX="7"
            refY="4"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path
              d="M0 0 L8 4 L0 8 z"
              fill={kind === "ok" ? "var(--foreground)" : "var(--signal)"}
            />
          </marker>
        ))}
      </defs>
      {arrows.map((arrow) => {
        const from = anchors.get(arrow.blocker);
        const to = anchors.get(arrow.blocked);
        if (!from || !to) return null;
        const [x1, y1, x2, y2] = [from.right, from.y, to.left, to.y];
        const out = x1 + 8;
        const into = x2 - 8;
        const mid = (y1 + y2) / 2;
        const points = [
          [x1, y1],
          [out, y1],
          [out, mid],
          [into, mid],
          [into, y2],
          [x2, y2],
        ]
          .map(([x, y]) => `${x},${y}`)
          .join(" ");
        return (
          <polyline
            key={arrow.id}
            points={points}
            fill="none"
            stroke={arrow.violated ? "var(--signal)" : "var(--foreground)"}
            strokeWidth={arrow.violated ? 1.6 : 1.2}
            opacity={0.8}
            markerEnd={`url(#arrow-${arrow.violated ? "late" : "ok"})`}
          />
        );
      })}
    </svg>
  );
}
