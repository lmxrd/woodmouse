import { useState, type ReactNode } from "react";
import type { RevisionSummary } from "woodmouse-client";

import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";

/**
 * A side panel (full screen on phones) listing earlier versions, newest first, with a preview of the
 * selected one and a Restore button. What a version looks like is up to the caller (`renderPreview`).
 */
export function HistoryPanel({
  open,
  onOpenChange,
  title,
  revisions,
  renderPreview,
  canRestore,
  restoring,
  onRestore,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  revisions: RevisionSummary[];
  renderPreview: (revisionId: string) => ReactNode;
  canRestore: boolean;
  restoring: boolean;
  onRestore: (revisionId: string) => void;
}) {
  const { when } = useFormatters();
  const [selected, setSelected] = useState<string | null>(null);
  const active = revisions.find((r) => r.id === selected)?.id ?? revisions[0]?.id;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        aria-describedby={undefined}
        className="inset-y-0 right-0 left-auto flex h-dvh flex-col max-w-none translate-x-0 translate-y-0 gap-0 rounded-none border-foreground p-0 sm:max-w-3xl"
      >
        <DialogDescription className="sr-only">Earlier versions</DialogDescription>
        <div className="flex items-center justify-between gap-2 border-b border-border px-4 py-3">
          <DialogTitle className="font-display text-lg">{title}</DialogTitle>
          <Button type="button" variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </div>

        {revisions.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">
            No earlier versions yet. A version is kept when someone else edits, or when editing
            resumes after a pause of ten minutes.
          </p>
        ) : (
          <div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] md:grid-cols-[240px_minmax(0,1fr)] md:grid-rows-1">
            <ul
              aria-label="Versions"
              className="max-h-44 overflow-y-auto border-b border-border md:max-h-none md:border-r md:border-b-0"
            >
              {revisions.map((revision) => (
                <li key={revision.id}>
                  <button
                    type="button"
                    aria-current={revision.id === active}
                    onClick={() => setSelected(revision.id)}
                    className={cn(
                      "flex w-full flex-col px-4 py-2 text-left text-sm hover:bg-accent",
                      revision.id === active && "bg-accent",
                    )}
                  >
                    <span>{revision.saved_at ? when(revision.saved_at) : "Original"}</span>
                    <span className="label-mono text-muted-foreground">
                      {revision.saved_by
                        ? userLabel(revision.saved_by)
                        : revision.saved_at
                          ? "Unknown"
                          : "As first written"}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex min-h-0 flex-col">
              {canRestore && active ? (
                <div className="flex items-center justify-between gap-2 border-b border-border px-4 py-2">
                  <span className="text-sm text-muted-foreground">
                    The current version is kept in this list.
                  </span>
                  <Button size="sm" disabled={restoring} onClick={() => onRestore(active)}>
                    Restore this version
                  </Button>
                </div>
              ) : null}
              <div className="min-h-0 flex-1 overflow-y-auto p-4">
                {active ? renderPreview(active) : null}
              </div>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
