import { useState } from "react";

import { Sidebar } from "./Sidebar";
import { Button } from "./ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

/** Phone-only top bar with a menu button that opens the sidebar as a left drawer. */
export function MobileNav({ workspaceId }: { workspaceId: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex items-center gap-2 border-b border-border bg-card px-4 py-2 md:hidden">
      <Button type="button" variant="ghost" size="sm" onClick={() => setOpen(true)}>
        ☰ Menu
      </Button>
      <span className="font-display text-sm font-bold">Woodmouse</span>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent
          showCloseButton={false}
          aria-describedby={undefined}
          className="inset-y-0 left-0 h-dvh max-w-[280px] translate-x-0 translate-y-0 gap-0 rounded-none p-0 sm:max-w-[280px]"
        >
          <DialogTitle className="sr-only">Menu</DialogTitle>
          <DialogDescription className="sr-only">Workspace and projects</DialogDescription>
          <Sidebar
            workspaceId={workspaceId}
            className="h-full border-r-0"
            onNavigate={() => setOpen(false)}
          />
        </DialogContent>
      </Dialog>
    </div>
  );
}
