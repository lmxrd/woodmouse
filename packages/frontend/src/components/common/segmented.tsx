import { cn } from "../../lib/utils";

/** A row of mutually exclusive choices, joined together. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div role="group" aria-label={label} className="inline-flex">
      {options.map((option, i) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={cn(
            "border border-border px-3 py-1.5 text-sm outline-none focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-ring",
            i > 0 && "-ml-px",
            value === option.value
              ? "border-foreground bg-secondary text-secondary-foreground"
              : "bg-card hover:bg-accent",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
