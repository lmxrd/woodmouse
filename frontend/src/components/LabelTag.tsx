import { cn } from "../lib/utils";

/** A small-caps label chip. */
export function LabelTag({ name, className }: { name: string; className?: string }) {
  return (
    <span
      className={cn(
        "bg-accent px-1.5 py-0.5 font-mono text-[10px] tracking-wide text-accent-foreground uppercase",
        className,
      )}
    >
      {name}
    </span>
  );
}
