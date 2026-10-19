import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { List } from "woodmouse-sdk";
import { createInboxTaskMutation, createTaskMutation } from "woodmouse-sdk/react-query";

import { dayIso } from "../../lib/time";
import { useFormatters } from "../../lib/useFormatters";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

/** The value of the "no list" choice: the project's inbox. */
const INBOX = "inbox";

/** A small "+" on a calendar day: add a task due that day, to the Inbox unless another list is picked. */
export function QuickAdd({
  day,
  projectId,
  lists,
}: {
  day: number;
  projectId: string;
  lists: List[];
}) {
  const { localDay } = useFormatters();
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [listId, setListId] = useState<string | undefined>(undefined);
  const create = useMutation({
    ...createTaskMutation(),
  });
  const createInbox = useMutation({
    ...createInboxTaskMutation(),
  });
  const target = listId ?? INBOX;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={`Add a task due ${localDay(day)}`}
          className="opacity-0 group-hover/day:opacity-100 focus-visible:opacity-100 max-md:hidden"
        >
          +
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 p-3">
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const name = title.trim();
            if (!name) return;
            const body = { title: name, due_date: dayIso(day) };
            if (target === INBOX) createInbox.mutate({ path: { project_id: projectId }, body });
            else create.mutate({ path: { list_id: target }, body });
            setTitle("");
            setOpen(false);
          }}
        >
          <p className="label-mono text-muted-foreground">Due {localDay(day)}</p>
          <Input
            autoFocus
            aria-label="Task title"
            placeholder="Task title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <Select value={target} onValueChange={setListId}>
            <SelectTrigger size="sm" aria-label="List" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={INBOX}>Inbox</SelectItem>
              {lists.map((l) => (
                <SelectItem key={l.id} value={l.id}>
                  {l.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button type="submit" size="sm" disabled={!title.trim()}>
            Add task
          </Button>
        </form>
      </PopoverContent>
    </Popover>
  );
}
