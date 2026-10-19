import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  deleteWorkspaceMutation,
  listWorkspacesOptions,
  updateWorkspaceMutation,
} from "woodmouse-client/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Field } from "../common/Field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";

/** The workspace's name and description, and (for admins) deleting it. */
export function GeneralPanel({ workspaceId }: { workspaceId: string }) {
  const navigate = useNavigate();
  const { isAdmin } = useWorkspaceRole(workspaceId);
  const { data: workspaces } = useQuery(listWorkspacesOptions());
  const workspace = workspaces?.find((w) => w.id === workspaceId);
  const update = useMutation({
    ...updateWorkspaceMutation(),
  });
  const remove = useMutation({
    ...deleteWorkspaceMutation(),
    onSuccess: async () => {
      void navigate({ to: "/" });
    },
  });
  const [confirming, setConfirming] = useState(false);
  if (!workspace) return null;

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6 p-4 sm:p-6">
      <form
        key={workspace.name + workspace.description}
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          update.mutate({
            path: { id: workspaceId },
            body: {
              name: String(data.get("name") ?? "").trim(),
              description: String(data.get("description") ?? "").trim(),
            },
          });
        }}
      >
        <Field label="Name" htmlFor="workspace-name">
          <Input
            id="workspace-name"
            name="name"
            required
            defaultValue={workspace.name}
            disabled={!isAdmin}
          />
        </Field>
        <Field label="Description" htmlFor="workspace-description">
          <Textarea
            id="workspace-description"
            name="description"
            rows={3}
            defaultValue={workspace.description}
            disabled={!isAdmin}
          />
        </Field>
        {isAdmin ? (
          <div>
            <Button type="submit" disabled={update.isPending}>
              Save
            </Button>
          </div>
        ) : null}
      </form>

      {isAdmin ? (
        <section className="flex flex-col gap-2 border border-destructive/40 p-4">
          <h2 className="label-mono text-destructive">Delete workspace</h2>
          <p className="text-sm text-muted-foreground">
            The workspace disappears for everyone. Nothing in it is destroyed: the site's
            administrator can bring it back.
          </p>
          <div>
            <Button variant="destructive" onClick={() => setConfirming(true)}>
              Delete workspace…
            </Button>
          </div>
        </section>
      ) : null}
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={`Delete "${workspace.name}"?`}
        description="Everyone loses access to it. Only the site's administrator can restore it."
        confirmLabel="Delete workspace"
        onConfirm={() => remove.mutate({ path: { id: workspaceId } })}
      />
    </div>
  );
}
