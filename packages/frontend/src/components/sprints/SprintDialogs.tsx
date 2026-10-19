import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { SprintView } from "woodmouse-sdk";
import {
  completeSprintMutation,
  createSprintMutation,
  updateSprintMutation,
} from "woodmouse-sdk/react-query";

import { fromDateInput, toDateInput } from "../../lib/dates";
import { Field } from "../common/Field";
import { ResponsiveDialog } from "../common/ResponsiveDialog";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Textarea } from "../ui/textarea";

const dayIn = (offset: number) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};

/** Create a sprint, or (with `sprint`) change its name, goal and dates. Two weeks starting today by default. */
export function SprintFormDialog({
  projectId,
  sprint,
  open,
  onOpenChange,
}: {
  projectId: string;
  sprint?: SprintView;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const create = useMutation(createSprintMutation());
  const update = useMutation(updateSprintMutation());
  const [error, setError] = useState<string | null>(null);

  return (
    <ResponsiveDialog
      open={open}
      onOpenChange={onOpenChange}
      title={sprint ? "Edit sprint" : "New sprint"}
    >
      <form
        key={sprint?.id ?? "new"}
        className="flex flex-col gap-4 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          const start = fromDateInput(String(data.get("start")));
          const end = fromDateInput(String(data.get("end")));
          const name = String(data.get("name") ?? "").trim();
          if (!start || !end || !name) return;
          if (end < start) {
            setError("A sprint cannot end before it starts.");
            return;
          }
          const body = {
            name,
            goal: String(data.get("goal") ?? ""),
            start_date: start,
            end_date: end,
          };
          const done = () => onOpenChange(false);
          if (sprint) update.mutate({ path: { id: sprint.id }, body }, { onSuccess: done });
          else create.mutate({ path: { project_id: projectId }, body }, { onSuccess: done });
        }}
      >
        <h2 className="font-display text-xl font-bold">{sprint ? "Edit sprint" : "New sprint"}</h2>
        <Field label="Name" htmlFor="sprint-name">
          <Input id="sprint-name" name="name" required defaultValue={sprint?.name} />
        </Field>
        <Field label="Goal (optional)" htmlFor="sprint-goal">
          <Textarea id="sprint-goal" name="goal" rows={2} defaultValue={sprint?.goal} />
        </Field>
        <div className="flex flex-wrap gap-4">
          <Field label="Starts" htmlFor="sprint-start">
            <Input
              id="sprint-start"
              name="start"
              type="date"
              required
              defaultValue={sprint ? toDateInput(sprint.start_date) : dayIn(0)}
            />
          </Field>
          <Field label="Ends" htmlFor="sprint-end">
            <Input
              id="sprint-end"
              name="end"
              type="date"
              required
              defaultValue={sprint ? toDateInput(sprint.end_date) : dayIn(13)}
            />
          </Field>
        </div>
        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        ) : null}
        <div className="flex gap-2">
          <Button type="submit" disabled={create.isPending || update.isPending}>
            {sprint ? "Save" : "Create sprint"}
          </Button>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        </div>
      </form>
    </ResponsiveDialog>
  );
}

const BACKLOG = "backlog";

/** Complete a sprint: choose where its unfinished tasks go (the backlog or another open sprint). */
export function CompleteSprintDialog({
  sprint,
  others,
  unfinished,
  open,
  onOpenChange,
}: {
  sprint: SprintView;
  /** The project's other open sprints. */
  others: SprintView[];
  unfinished: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [target, setTarget] = useState(BACKLOG);
  const complete = useMutation({
    ...completeSprintMutation(),
    onSuccess: async () => {
      onOpenChange(false);
    },
  });

  return (
    <ResponsiveDialog open={open} onOpenChange={onOpenChange} title="Complete sprint">
      <div className="flex flex-col gap-4 p-5">
        <h2 className="font-display text-xl font-bold">Complete “{sprint.name}”?</h2>
        <p className="text-sm text-muted-foreground">
          Finished tasks stay in the sprint as its record.{" "}
          {unfinished === 0
            ? "Everything in it is done."
            : `${unfinished} unfinished ${unfinished === 1 ? "task" : "tasks"} will move:`}
        </p>
        {unfinished > 0 ? (
          <Select value={target} onValueChange={setTarget}>
            <SelectTrigger aria-label="Move unfinished tasks to" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={BACKLOG}>To the backlog</SelectItem>
              {others.map((s) => (
                <SelectItem key={s.id} value={s.id}>
                  To {s.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}
        <div className="flex gap-2">
          <Button
            disabled={complete.isPending}
            onClick={() =>
              complete.mutate({
                path: { id: sprint.id },
                body: { move_to: target === BACKLOG ? null : target },
              })
            }
          >
            Complete sprint
          </Button>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        </div>
      </div>
    </ResponsiveDialog>
  );
}
