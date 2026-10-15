import type { ReactNode } from "react";

import { Label } from "./ui/label";

/** The standalone ruled card used by login, first-run setup and invite acceptance. */
export function AuthSheet({
  tag,
  title,
  description,
  children,
}: {
  tag: string;
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <main className="grid min-h-screen place-items-center px-4 py-10">
      <section className="w-full max-w-md border border-border bg-card">
        <header className="flex items-center justify-between border-b-2 border-foreground px-6 py-3">
          <span className="font-display text-lg font-bold">Woodmouse</span>
          <span className="label-mono text-[10px] text-muted-foreground">{tag}</span>
        </header>
        <div className="flex flex-col gap-5 p-6">
          <div className="flex flex-col gap-2">
            <h1 className="font-display text-3xl leading-tight font-bold">{title}</h1>
            {description ? (
              <p className="max-w-[44ch] text-sm leading-6 text-muted-foreground">{description}</p>
            ) : null}
          </div>
          {children}
        </div>
        <footer className="label-mono border-t border-border px-6 py-2.5 text-[10px] text-muted-foreground">
          Self-hosted
        </footer>
      </section>
    </main>
  );
}

export function AuthField({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id} className="label-mono text-[11px] text-muted-foreground">
        {label}
      </Label>
      {children}
      {hint ? <p className="font-mono text-[11px] text-muted-foreground">{hint}</p> : null}
    </div>
  );
}

export function ErrorNote({ message }: { message: string }) {
  return (
    <div role="alert" className="flex gap-3 border border-destructive bg-card px-3 py-2.5 text-sm">
      <span className="font-mono font-medium text-destructive">ERR</span>
      <span>{message}</span>
    </div>
  );
}
