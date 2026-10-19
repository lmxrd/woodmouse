import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { LabelWithCount } from "woodmouse-sdk";
import {
  createLabelMutation,
  deleteLabelMutation,
  listLabelsOptions,
  updateLabelMutation,
} from "woodmouse-sdk/react-query";

import { LABEL_COLORS } from "../../lib/labelColors";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { InlineAdd } from "../common/InlineAdd";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** Workspace labels: rename, recolor from a fixed palette, delete. Editing needs write access. */
export function LabelsPanel({ workspaceId }: { workspaceId: string }) {
  const { data: labels } = useQuery(listLabelsOptions({ path: { workspace_id: workspaceId } }));
  const { canWrite: canEdit } = useWorkspaceRole(workspaceId);

  const create = useMutation(createLabelMutation());
  const update = useMutation(updateLabelMutation());
  const remove = useMutation(deleteLabelMutation());
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const row = (label: LabelWithCount) => (
    <li key={label.id} className="flex items-center gap-3 border-b border-border px-4 py-2.5">
      {canEdit ? (
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              aria-label={`Color of ${label.name}`}
              className="size-4 shrink-0 ring-offset-2 ring-offset-card hover:ring-2 hover:ring-ring"
              style={{ backgroundColor: label.color }}
            />
          </PopoverTrigger>
          <PopoverContent align="start" className="flex w-auto gap-1.5 p-2">
            {LABEL_COLORS.map((color) => (
              <button
                key={color}
                type="button"
                aria-label={color}
                className="size-6 hover:ring-2 hover:ring-ring"
                style={{ backgroundColor: color }}
                onClick={() => update.mutate({ path: { id: label.id }, body: { color } })}
              />
            ))}
          </PopoverContent>
        </Popover>
      ) : (
        <span className="size-4 shrink-0" style={{ backgroundColor: label.color }} />
      )}
      {canEdit ? (
        <Input
          key={label.name}
          aria-label={`Name of ${label.name}`}
          defaultValue={label.name}
          className="h-8 flex-1 border-transparent bg-transparent hover:border-input"
          onBlur={(e) => {
            const name = e.target.value.trim();
            if (!name) e.target.value = label.name;
            else if (name !== label.name) update.mutate({ path: { id: label.id }, body: { name } });
          }}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        />
      ) : (
        <span className="flex-1 text-sm font-medium">{label.name}</span>
      )}
      <span className="label-mono text-muted-foreground">
        {label.task_count} {label.task_count === 1 ? "task" : "tasks"}
      </span>
      {canEdit ? (
        confirmDelete === label.id ? (
          <span className="flex items-center gap-1">
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => {
                remove.mutate({ path: { id: label.id } });
                setConfirmDelete(null);
              }}
            >
              Delete
            </Button>
            <Button type="button" variant="ghost" size="sm" onClick={() => setConfirmDelete(null)}>
              Keep
            </Button>
          </span>
        ) : (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-destructive"
            onClick={() => setConfirmDelete(label.id)}
          >
            Delete…
          </Button>
        )
      ) : null}
    </li>
  );

  return (
    <div className="flex flex-col">
      <ul className="bg-card">{labels?.map(row)}</ul>
      {labels?.length === 0 ? (
        <p className="p-4 text-sm text-muted-foreground sm:px-6">No labels yet.</p>
      ) : null}
      {canEdit ? (
        <div className="p-4 sm:px-6">
          <InlineAdd
            className="max-w-xs"
            label="New label"
            placeholder="Label name"
            onSubmit={(name) =>
              create.mutate({ path: { workspace_id: workspaceId }, body: { name } })
            }
          />
        </div>
      ) : null}
    </div>
  );
}
