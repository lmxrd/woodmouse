import type { Member, PublicUser } from "woodmouse-client";

import { userLabel } from "../../lib/user";
import { UserAvatar } from "../UserAvatar";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** Assignee chips plus a popover to tick workspace members. */
export function MemberPicker({
  members,
  selected,
  onChange,
}: {
  members: Member[];
  selected: PublicUser[];
  onChange: (userIds: string[]) => void;
}) {
  const selectedIds = new Set(selected.map((u) => u.id));
  const toggle = (id: string) =>
    onChange(selectedIds.has(id) ? [...selectedIds].filter((x) => x !== id) : [...selectedIds, id]);

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {selected.map((user) => (
        <UserAvatar key={user.id} name={userLabel(user)} size="md" />
      ))}
      <Popover>
        <PopoverTrigger asChild>
          <Button type="button" variant="outline" size="xs" aria-label="Edit assignees">
            {selected.length === 0 ? "Assign" : "+"}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-56 p-1">
          {members.map(({ user }) => (
            <label
              key={user.id}
              className="flex cursor-pointer items-center gap-2 px-2 py-1.5 text-sm hover:bg-accent"
            >
              <Checkbox
                checked={selectedIds.has(user.id)}
                onCheckedChange={() => toggle(user.id)}
              />
              <span className="min-w-0 flex-1 truncate">{userLabel(user)}</span>
            </label>
          ))}
        </PopoverContent>
      </Popover>
    </div>
  );
}
