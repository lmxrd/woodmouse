import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { LinkKind, TaskLinkView } from "woodmouse-client";
import {
  addLinkMutation,
  listLinksOptions,
  listTasksOptions,
  removeLinkMutation,
} from "woodmouse-client/react-query";

import { Button } from "../ui/button";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "../ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

/** What the user picks: a kind and which end of it this task is. */
const CHOICES = [
  { value: "blocks", label: "Blocks", kind: "blocks", owns: true },
  { value: "blocked_by", label: "Blocked by", kind: "blocks", owns: false },
  { value: "relates_to", label: "Relates to", kind: "relates_to", owns: true },
  { value: "duplicates", label: "Duplicates", kind: "duplicates", owns: true },
  { value: "duplicated_by", label: "Duplicated by", kind: "duplicates", owns: false },
] as const satisfies readonly { value: string; label: string; kind: LinkKind; owns: boolean }[];

export function linkLabel(link: Pick<TaskLinkView, "kind" | "direction">): string {
  const outgoing = link.direction === "outgoing";
  const choice = CHOICES.find((c) => c.kind === link.kind && c.owns === outgoing) ?? CHOICES[2];
  return choice.label;
}

/** Links to other tasks (blocks, relates to, duplicates), with a searchable picker to add more. */
export function TaskLinks({
  taskId,
  workspaceId,
  editable,
  onOpenTask,
}: {
  taskId: string;
  workspaceId: string;
  editable: boolean;
  onOpenTask: (taskId: string) => void;
}) {
  const queryClient = useQueryClient();
  const { data: links } = useQuery(listLinksOptions({ path: { id: taskId } }));
  const refresh = () => queryClient.invalidateQueries();
  const add = useMutation({
    ...addLinkMutation(),
    onSuccess: refresh,
    meta: { inline: true },
    onError: () => toast.error("Those tasks are already linked that way."),
  });
  const remove = useMutation({ ...removeLinkMutation(), onSuccess: refresh });
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState<(typeof CHOICES)[number]["value"]>("relates_to");
  const { data: candidates } = useQuery({
    ...listTasksOptions({
      path: { workspace_id: workspaceId },
      query: { include_subtasks: true },
    }),
    enabled: open,
  });

  if ((links?.length ?? 0) === 0 && !editable) return null;

  return (
    <section aria-label="Related tasks" className="flex flex-col gap-1">
      <h2 className="label-mono text-muted-foreground">Related</h2>
      {links?.map((link) => (
        <div key={link.id} className="flex items-center gap-3 border-b border-border py-1.5">
          <span className="label-mono w-24 shrink-0">{linkLabel(link)}</span>
          <button
            type="button"
            onClick={() => onOpenTask(link.task.id)}
            className="min-w-0 flex-1 truncate text-left text-sm hover:underline"
          >
            {link.task.title}
          </button>
          {editable ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label={`Remove link to ${link.task.title}`}
              onClick={() => remove.mutate({ path: { id: taskId, link_id: link.id } })}
            >
              ✕
            </Button>
          ) : null}
        </div>
      ))}
      {editable ? (
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="justify-start font-normal text-muted-foreground"
            >
              Link a task…
            </Button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-80 p-2">
            <Select value={choice} onValueChange={(v) => setChoice(v as typeof choice)}>
              <SelectTrigger size="sm" aria-label="Link type" className="mb-2 w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {CHOICES.map((c) => (
                  <SelectItem key={c.value} value={c.value}>
                    {c.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Command>
              <CommandInput placeholder="Search tasks…" />
              <CommandList>
                <CommandEmpty>No tasks found.</CommandEmpty>
                {candidates
                  ?.filter((t) => t.id !== taskId)
                  .map((t) => (
                    <CommandItem
                      key={t.id}
                      value={`${t.title} ${t.id}`}
                      onSelect={() => {
                        const picked = CHOICES.find((c) => c.value === choice) ?? CHOICES[2];
                        const [from, to] = picked.owns ? [taskId, t.id] : [t.id, taskId];
                        add.mutate({
                          path: { id: from },
                          body: { related_task_id: to, kind: picked.kind },
                        });
                        setOpen(false);
                      }}
                    >
                      {t.title}
                    </CommandItem>
                  ))}
              </CommandList>
            </Command>
          </PopoverContent>
        </Popover>
      ) : null}
    </section>
  );
}
