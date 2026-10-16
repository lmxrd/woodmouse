import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { AdminUser, Role } from "woodmouse-client";
import {
  addMemberMutation,
  removeMemberMutation,
  updateMemberMutation,
} from "woodmouse-client/react-query";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

export const NO_ACCESS = "none";

/**
 * Set a person's role in a workspace from the site admin: adds, changes or removes the membership as
 * needed (`null` removes it). The server keeps the rule that a workspace needs an admin.
 */
export function useSetMembership() {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries();
  const add = useMutation({ ...addMemberMutation(), onSuccess: refresh });
  const update = useMutation({ ...updateMemberMutation(), onSuccess: refresh });
  const remove = useMutation({ ...removeMemberMutation(), onSuccess: refresh });
  return {
    pending: add.isPending || update.isPending || remove.isPending,
    set: (user: AdminUser, workspaceId: string, role: Role | null) => {
      const current = user.memberships.find((m) => m.workspace_id === workspaceId);
      if (role === null) {
        if (current) remove.mutate({ path: { id: workspaceId, user_id: user.id } });
      } else if (current) {
        if (current.role !== role) {
          update.mutate({ path: { id: workspaceId, user_id: user.id }, body: { role } });
        }
      } else {
        add.mutate({ path: { id: workspaceId }, body: { email: user.email, role } });
      }
    },
  };
}

/** None, read, write or admin. */
export function RoleSelect({
  value,
  onChange,
  label,
  noneLabel = "No access",
  className = "w-32",
}: {
  value: Role | null;
  onChange: (role: Role | null) => void;
  label: string;
  noneLabel?: string;
  className?: string;
}) {
  return (
    <Select
      value={value ?? NO_ACCESS}
      onValueChange={(next) => onChange(next === NO_ACCESS ? null : (next as Role))}
    >
      <SelectTrigger aria-label={label} size="sm" className={className}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NO_ACCESS}>{noneLabel}</SelectItem>
        <SelectItem value="read">Read</SelectItem>
        <SelectItem value="write">Write</SelectItem>
        <SelectItem value="admin">Admin</SelectItem>
      </SelectContent>
    </Select>
  );
}
