import type { FileView } from "woodmouse-client";

import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";

/** Shown when an upload has the same content as a file already in the library. */
export function DuplicateFileDialog({
  uploadedName,
  existing,
  attachesToTask,
  onUseExisting,
  onCancel,
}: {
  uploadedName: string;
  existing: FileView;
  /** True when the upload was meant for a task, so "use existing" attaches the library file to it. */
  attachesToTask: boolean;
  onUseExisting: () => void;
  onCancel: () => void;
}) {
  const { dateTime } = useFormatters();
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent showCloseButton={false} className="rounded-none">
        <DialogHeader>
          <DialogTitle className="font-display text-xl">This file is already here</DialogTitle>
          <DialogDescription>
            {uploadedName} has the same content as <b className="font-medium">{existing.name}</b>,
            uploaded{existing.uploaded_by ? ` by ${userLabel(existing.uploaded_by)}` : ""} on{" "}
            {dateTime(existing.created_at)} and used in {existing.used_in}{" "}
            {existing.used_in === 1 ? "task" : "tasks"}.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="button" onClick={onUseExisting}>
            {attachesToTask ? "Use existing file" : "OK"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
