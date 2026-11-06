import type { ReactNode } from "react";

/** A small-caps caption above a control. Pass `htmlFor` when the control is a native input. */
export function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={htmlFor} className="label-mono text-muted-foreground">
        {label}
      </label>
      {children}
    </div>
  );
}
