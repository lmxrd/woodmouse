import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { CalendarLinkView, LinkScope } from "woodmouse-client";
import {
  createCalendarLinkMutation,
  listProjectsOptions,
  listWorkspacesOptions,
} from "woodmouse-client/react-query";

import { cn } from "../../lib/utils";
import { Field } from "../Field";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const CHOICES: Record<LinkScope, { label: string; hint: string }> = {
  user: { label: "My tasks", hint: "Tasks assigned to you, in every workspace" },
  project: { label: "One project", hint: "Every task with a date in the project" },
  workspace: { label: "A whole workspace", hint: "Every task with a date in the workspace" },
};

/** Make a calendar link. The scopes offered, and what is already chosen, depend on where it opens. */
export function NewCalendarLink({
  open,
  onOpenChange,
  workspaceId,
  projectId,
  scopes,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceId?: string;
  projectId?: string;
  scopes: LinkScope[];
  onCreated: (link: CalendarLinkView) => void;
}) {
  const [scope, setScope] = useState<LinkScope>(scopes[0]);
  const [chosenWorkspace, setChosenWorkspace] = useState<string | undefined>(workspaceId);
  const [chosenProject, setChosenProject] = useState<string | undefined>(projectId);
  const [name, setName] = useState("");

  const { data: workspaces = [] } = useQuery({ ...listWorkspacesOptions(), enabled: open });
  const wsId = chosenWorkspace ?? workspaces[0]?.id;
  const { data: projects = [] } = useQuery({
    ...listProjectsOptions({ path: { workspace_id: wsId ?? "" } }),
    enabled: open && scope === "project" && !!wsId && !projectId,
  });

  const create = useMutation({
    ...createCalendarLinkMutation(),
    onSuccess: (link) => {
      onCreated(link);
      setName("");
      onOpenChange(false);
    },
  });

  const projectChoice = projectId ?? chosenProject ?? projects[0]?.id;
  const ready =
    scope === "user" ||
    (scope === "workspace" && !!wsId) ||
    (scope === "project" && !!projectChoice);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="rounded-none">
        <DialogHeader>
          <DialogTitle>New calendar link</DialogTitle>
          <DialogDescription>
            A read-only calendar feed you can subscribe to from any calendar app.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (!ready) return;
            create.mutate({
              body: {
                scope,
                name: name.trim() || undefined,
                workspace_id: scope === "user" ? undefined : wsId,
                project_id: scope === "project" ? projectChoice : undefined,
              },
            });
          }}
        >
          {scopes.length > 1 ? (
            <fieldset className="flex flex-col gap-1.5">
              <legend className="label-mono mb-1 text-muted-foreground">What should it show</legend>
              {scopes.map((s) => (
                <label
                  key={s}
                  className={cn(
                    "flex cursor-pointer flex-col border px-3 py-2 text-sm",
                    scope === s
                      ? "border-foreground bg-card"
                      : "border-border text-muted-foreground",
                  )}
                >
                  <span className="flex items-center gap-2">
                    <input
                      type="radio"
                      name="calendar-scope"
                      checked={scope === s}
                      onChange={() => setScope(s)}
                    />
                    {CHOICES[s].label}
                  </span>
                  <span className="pl-6 text-xs text-muted-foreground">{CHOICES[s].hint}</span>
                </label>
              ))}
            </fieldset>
          ) : null}

          {!workspaceId && scope !== "user" ? (
            <Field label="Workspace" htmlFor="calendar-workspace">
              <Select
                value={wsId}
                onValueChange={(value) => {
                  setChosenWorkspace(value);
                  setChosenProject(undefined);
                }}
              >
                <SelectTrigger id="calendar-workspace">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {workspaces.map((w) => (
                    <SelectItem key={w.id} value={w.id}>
                      {w.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          ) : null}
          {scope === "project" && !projectId ? (
            <Field label="Project" htmlFor="calendar-project">
              <Select value={projectChoice} onValueChange={setChosenProject}>
                <SelectTrigger id="calendar-project">
                  <SelectValue placeholder="No projects" />
                </SelectTrigger>
                <SelectContent>
                  {projects.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          ) : null}

          <Field label="Name (optional)" htmlFor="calendar-name">
            <Input
              id="calendar-name"
              value={name}
              maxLength={100}
              placeholder="Phone calendar"
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!ready || create.isPending}>
              Create and copy link
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
