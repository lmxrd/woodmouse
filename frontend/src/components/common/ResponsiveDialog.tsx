import type { ReactNode } from "react";

import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";

/**
 * A dialog that fills the screen on phones and is a centered panel from `sm` up.
 * The one place that decides how detail views are presented; `title` is read by screen readers only.
 */
export function ResponsiveDialog({
  open,
  onOpenChange,
  title,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: ReactNode;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        aria-describedby={undefined}
        className="inset-0 h-dvh max-h-dvh max-w-none translate-x-0 translate-y-0 gap-0 overflow-y-auto rounded-none border-foreground bg-card p-0 sm:inset-auto sm:top-[50%] sm:left-[50%] sm:h-auto sm:max-h-[90dvh] sm:max-w-4xl sm:translate-x-[-50%] sm:translate-y-[-50%]"
      >
        <DialogTitle className="sr-only">{title}</DialogTitle>
        <DialogDescription className="sr-only">{title}</DialogDescription>
        {children}
      </DialogContent>
    </Dialog>
  );
}
