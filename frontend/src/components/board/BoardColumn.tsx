import { useState, type ReactNode } from "react";

import { cn } from "../../lib/utils";

/** A board column: a header, a stack of cards, and a drop target for dragged tasks. */
export function BoardColumn({
  label,
  header,
  onDropTask,
  children,
}: {
  label: string;
  header: ReactNode;
  /** Called with the dragged task's id; omit to make the column read-only. */
  onDropTask?: (taskId: string) => void;
  children: ReactNode;
}) {
  const [over, setOver] = useState(false);
  return (
    <section
      aria-label={label}
      className={cn(
        "flex w-[78vw] shrink-0 snap-start flex-col gap-2.5 pb-2 sm:w-[232px]",
        over && "bg-accent/50",
      )}
      onDragOver={(e) => {
        if (!onDropTask) return;
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        const taskId = e.dataTransfer.getData("text/plain");
        if (onDropTask && taskId) onDropTask(taskId);
      }}
    >
      <div className="flex items-center gap-1 border-b-2 border-foreground pb-1">{header}</div>
      {children}
    </section>
  );
}
