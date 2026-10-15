import { cn } from "../lib/utils";

/** A small-caps label chip, with a dot in the label's color when it has one. */
export function LabelTag({
  name,
  color,
  className,
}: {
  name: string;
  color?: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 bg-accent px-1.5 py-0.5 font-mono text-[10px] tracking-wide text-accent-foreground uppercase",
        className,
      )}
    >
      {color ? (
        <span aria-hidden className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
      ) : null}
      {name}
    </span>
  );
}
