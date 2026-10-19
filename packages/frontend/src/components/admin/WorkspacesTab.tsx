import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import {
  adminListWorkspacesOptions,
  adminRestoreWorkspaceMutation,
  createWorkspaceMutation,
} from "woodmouse-sdk/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { Field } from "../common/Field";
import { ResponsiveDialog } from "../common/ResponsiveDialog";
import { Button } from "../ui/button";
import { Segmented } from "../common/Segmented";
import { GeneralPanel } from "../settings/GeneralPanel";
import { MembersPanel } from "../settings/MembersPanel";
import { Input } from "../ui/input";
import { MembersMatrix } from "./MembersMatrix";
import { Textarea } from "../ui/textarea";

/** Every workspace on the site; deleted ones show behind a toggle and can be restored. */
export function WorkspacesTab({
  showDeleted,
  onShowDeleted,
  view,
  onView,
}: {
  showDeleted: boolean;
  onShowDeleted: (show: boolean) => void;
  view: "list" | "matrix";
  onView: (view: "list" | "matrix") => void;
}) {
  const navigate = useNavigate();
  const { dateTime } = useFormatters();
  const { data: workspaces } = useQuery(adminListWorkspacesOptions());
  const [creating, setCreating] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const restore = useMutation({
    ...adminRestoreWorkspaceMutation(),
  });
  const create = useMutation({
    ...createWorkspaceMutation(),
    onSuccess: async (workspace) => {
      setCreating(false);
      void navigate({
        to: "/settings/admin/workspaces/$workspaceId",
        params: { workspaceId: workspace.id },
      });
    },
  });
  const shown = workspaces?.filter((w) => showDeleted || !w.deleted_at) ?? [];

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-display text-xl font-bold">Workspaces</h2>
        <div className="flex flex-wrap items-center gap-4">
          <Segmented
            label="View"
            value={view}
            onChange={onView}
            options={[
              { value: "list", label: "List" },
              { value: "matrix", label: "Access matrix" },
            ]}
          />
          {view === "list" ? (
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={showDeleted}
                onChange={(e) => onShowDeleted(e.target.checked)}
              />
              Show deleted
            </label>
          ) : null}
          <Button onClick={() => setCreating(true)}>New workspace</Button>
        </div>
      </div>
      {view === "matrix" ? (
        <MembersMatrix />
      ) : shown.length === 0 ? (
        <p className="border border-dashed border-border px-4 py-6 text-muted-foreground">
          No workspaces.
        </p>
      ) : (
        <div className="flex flex-col border border-border bg-card">
          {shown.map((w) => (
            <div key={w.id} className="border-b border-border last:border-b-0">
              <div className="flex items-center gap-3 px-4 py-3">
                <div className="min-w-0 flex-1">
                  {w.deleted_at ? (
                    <>
                      <p className="truncate font-medium text-muted-foreground">{w.name}</p>
                      <p className="label-mono text-muted-foreground">
                        Deleted {dateTime(w.deleted_at)}
                      </p>
                    </>
                  ) : (
                    <>
                      <p className="truncate font-medium">{w.name}</p>
                      <p className="label-mono text-muted-foreground">
                        {w.members} {w.members === 1 ? "member" : "members"} · {w.projects}{" "}
                        {w.projects === 1 ? "project" : "projects"}
                      </p>
                    </>
                  )}
                </div>
                {w.deleted_at ? (
                  <Button size="sm" onClick={() => restore.mutate({ path: { id: w.id } })}>
                    Restore
                  </Button>
                ) : (
                  <Button
                    variant="outline"
                    size="sm"
                    aria-expanded={expanded === w.id}
                    onClick={() => setExpanded(expanded === w.id ? null : w.id)}
                  >
                    Manage {expanded === w.id ? "▴" : "▾"}
                  </Button>
                )}
              </div>
              {expanded === w.id && !w.deleted_at ? (
                <div className="flex flex-col gap-2 border-t border-border bg-muted/40 pb-3">
                  <GeneralPanel workspaceId={w.id} />
                  <MembersPanel workspaceId={w.id} />
                  <div className="px-4">
                    <Link
                      to="/settings/admin/workspaces/$workspaceId"
                      params={{ workspaceId: w.id }}
                      className="label-mono text-muted-foreground hover:text-foreground"
                    >
                      All settings (labels, calendar, appearance) →
                    </Link>
                  </div>
                </div>
              ) : null}
            </div>
          ))}
        </div>
      )}
      <ResponsiveDialog open={creating} onOpenChange={setCreating} title="New workspace">
        <form
          className="flex flex-col gap-4 p-5"
          onSubmit={(e) => {
            e.preventDefault();
            const data = new FormData(e.currentTarget);
            create.mutate({
              body: {
                name: String(data.get("name") ?? "").trim(),
                description: String(data.get("description") ?? "").trim(),
              },
            });
          }}
        >
          <h2 className="font-display text-xl font-bold">New workspace</h2>
          <Field label="Name" htmlFor="new-workspace-name">
            <Input id="new-workspace-name" name="name" required />
          </Field>
          <Field label="Description" htmlFor="new-workspace-description">
            <Textarea id="new-workspace-description" name="description" rows={3} />
          </Field>
          <div className="flex gap-2">
            <Button type="submit" disabled={create.isPending}>
              Create
            </Button>
            <Button type="button" variant="ghost" onClick={() => setCreating(false)}>
              Cancel
            </Button>
          </div>
        </form>
      </ResponsiveDialog>
    </section>
  );
}
