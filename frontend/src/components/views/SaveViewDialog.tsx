import { useState } from "react";

import { Field } from "../Field";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";

/** Asks for a name (and whether to share) before saving the current view settings. */
export function SaveViewDialog({
  open,
  onOpenChange,
  canShare,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  canShare: boolean;
  onSave: (name: string, isShared: boolean) => void;
}) {
  const [name, setName] = useState("");
  const [shared, setShared] = useState(false);
  const submit = () => {
    if (!name.trim()) return;
    onSave(name.trim(), shared && canShare);
    setName("");
    setShared(false);
    onOpenChange(false);
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent showCloseButton={false} className="rounded-none">
        <DialogHeader>
          <DialogTitle className="font-display text-xl">Save this view</DialogTitle>
          <DialogDescription>
            Keeps the current view, filters and sorting under a name you can come back to.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Field label="Name" htmlFor="view-name">
            <Input
              id="view-name"
              autoFocus
              value={name}
              placeholder="Due this week"
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          {canShare ? (
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={shared} onCheckedChange={(c) => setShared(c === true)} />
              Share with everyone in the workspace
            </label>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim()}>
              Save view
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
