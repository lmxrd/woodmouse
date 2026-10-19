import { useMutation } from "@tanstack/react-query";
import type { SavedViewResponse } from "woodmouse-client";
import { deleteViewMutation, updateViewMutation } from "woodmouse-client/react-query";

import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";
import { RenameInput } from "../common/RenameInput";
import { useState } from "react";
import { userLabel } from "../../lib/user";

/** Rename, share, update or delete saved views. Deleted views can be restored from the trash. */
export function ManageViewsDialog({
  open,
  onOpenChange,
  views,
  canShare,
  currentParams,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  views: SavedViewResponse[];
  canShare: boolean;
  /** The view settings in effect now, for "Update to current". */
  currentParams: Record<string, string>;
}) {
  const update = useMutation(updateViewMutation());
  const remove = useMutation(deleteViewMutation());
  const [renaming, setRenaming] = useState<string | null>(null);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="rounded-none sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="font-display text-xl">Saved views</DialogTitle>
          <DialogDescription>
            Shared views are visible to the whole workspace. Deleted views go to the trash.
          </DialogDescription>
        </DialogHeader>
        {views.length === 0 ? (
          <p className="text-sm text-muted-foreground">No saved views yet.</p>
        ) : (
          <ul className="divide-y divide-border border border-border">
            {views.map((view) => (
              <li key={view.id} className="flex flex-wrap items-center gap-2 px-3 py-2">
                <div className="min-w-0 flex-1 basis-40">
                  {renaming === view.id ? (
                    <RenameInput
                      label={`Rename ${view.name}`}
                      value={view.name}
                      className="h-8"
                      onCancel={() => setRenaming(null)}
                      onSave={(name) => {
                        update.mutate({ path: { id: view.id }, body: { name } });
                        setRenaming(null);
                      }}
                    />
                  ) : (
                    <>
                      <p className="truncate text-sm font-medium">{view.name}</p>
                      <p className="label-mono text-muted-foreground">
                        {view.is_shared ? "Shared" : "Private"}
                        {view.owner ? ` · ${userLabel(view.owner)}` : ""}
                      </p>
                    </>
                  )}
                </div>
                {view.can_edit ? (
                  <div className="flex flex-wrap items-center gap-1">
                    {canShare ? (
                      <label className="flex items-center gap-1.5 px-1 text-xs">
                        <Checkbox
                          checked={view.is_shared}
                          onCheckedChange={(c) =>
                            update.mutate({
                              path: { id: view.id },
                              body: { is_shared: c === true },
                            })
                          }
                        />
                        Shared
                      </label>
                    ) : null}
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      onClick={() => setRenaming(view.id)}
                    >
                      Rename
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      onClick={() =>
                        update.mutate({ path: { id: view.id }, body: { params: currentParams } })
                      }
                    >
                      Use current filters
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="text-destructive"
                      onClick={() => remove.mutate({ path: { id: view.id } })}
                    >
                      Delete
                    </Button>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
