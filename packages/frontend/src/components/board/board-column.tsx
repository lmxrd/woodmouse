import type { ReactNode } from "react";
import { useDroppable } from "@dnd-kit/core";

import { cn } from "../../lib/utils";

/**
 * A board column: a header and a stack of cards. It is a drop target as a whole, so a card can be
 * dropped on an empty column or below the last card (`droppable` false makes it read-only).
 */
export function BoardColumn({
  id,
  label,
  header,
  droppable = true,
  children,
}: {
  id: string;
  label: string;
  header: ReactNode;
  droppable?: boolean;
  children: ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `col:${id}`, disabled: !droppable });
  return (
    <section
      ref={setNodeRef}
      aria-label={label}
      className={cn(
        "flex w-[78vw] shrink-0 snap-start flex-col gap-2.5 pb-2 sm:w-[232px]",
        isOver && "bg-accent/50",
      )}
    >
      <div className="flex items-center gap-1 border-b-2 border-foreground pb-1">{header}</div>
      {children}
    </section>
  );
}
