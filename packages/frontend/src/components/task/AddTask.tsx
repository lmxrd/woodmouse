import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { CreateTaskRequest, List } from "woodmouse-sdk";
import { createInboxTaskMutation, createTaskMutation } from "woodmouse-sdk/react-query";

import { InlineAdd } from "../common/InlineAdd";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

/** The value of the "no list" choice: the project's inbox. */
const INBOX = "inbox";

/**
 * A quick "add task" for the list and timeline views: pick where it goes (the inbox or a list) and type a
 * title. `defaultListId` preselects a list (for example the one filtered to); `extra` adds fields to the new
 * task, such as dates.
 */
export function AddTask({
  projectId,
  lists,
  defaultListId,
  extra,
  className,
}: {
  projectId: string;
  lists: List[];
  defaultListId?: string;
  extra?: Partial<CreateTaskRequest>;
  className?: string;
}) {
  const createInList = useMutation(createTaskMutation());
  const createInInbox = useMutation(createInboxTaskMutation());
  const [picked, setPicked] = useState<string | undefined>(undefined);
  const target = picked ?? (lists.some((l) => l.id === defaultListId) ? defaultListId! : INBOX);
  const name = target === INBOX ? "Inbox" : (lists.find((l) => l.id === target)?.name ?? "Inbox");

  return (
    <div className={className ?? "flex flex-wrap items-center gap-2 px-4 py-3 sm:px-6"}>
      <Select value={target} onValueChange={setPicked}>
        <SelectTrigger size="sm" aria-label="Add to" className="w-40">
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
      <InlineAdd
        className="w-full max-w-sm"
        label={`Add task to ${name}`}
        placeholder="Task title"
        onSubmit={(title) => {
          const body = { ...extra, title };
          if (target === INBOX) createInInbox.mutate({ path: { project_id: projectId }, body });
          else createInList.mutate({ path: { list_id: target }, body });
        }}
      />
    </div>
  );
}
