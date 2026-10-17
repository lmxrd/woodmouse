import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Member, Role } from "woodmouse-client";
import {
  addMemberMutation,
  reissueInviteMutation,
  removeMemberMutation,
  updateMemberMutation,
} from "woodmouse-client/react-query";
import { listMembersOptions, meOptions } from "woodmouse-client/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { userLabel } from "../../lib/user";
import { Field } from "../Field";
import { UserAvatar } from "../UserAvatar";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { InviteLinkBox } from "./InviteLinkBox";

const ROLES: Role[] = ["admin", "write", "read"];
const ROLE_NAME: Record<Role, string> = { admin: "Admin", write: "Write", read: "Read" };

function RoleSelect({
  value,
  onChange,
  label,
}: {
  value: Role;
  onChange: (role: Role) => void;
  label: string;
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Role)}>
      <SelectTrigger size="sm" aria-label={label} className="w-28">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {ROLES.map((r) => (
          <SelectItem key={r} value={r}>
            {ROLE_NAME[r]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** Everyone sees the member list; admins can add, change roles, remove, and reissue invite links. */
export function MembersPanel({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries();
  const { data: me } = useQuery(meOptions());
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const { isAdmin } = useWorkspaceRole(workspaceId);

  const [invite, setInvite] = useState<{ name: string; token: string } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const add = useMutation({
    ...addMemberMutation(),
    meta: { inline: true },
    onSuccess: (result) => {
      void refresh();
      setNotice(result.invite_token ? null : `${userLabel(result.member.user)} was added.`);
      setInvite(
        result.invite_token
          ? { name: userLabel(result.member.user), token: result.invite_token }
          : null,
      );
    },
  });
  const changeRole = useMutation({
    ...updateMemberMutation(),
    meta: { inline: true },
  });
  const remove = useMutation({
    ...removeMemberMutation(),
    meta: { inline: true },
    onSuccess: () => {
      setConfirmRemove(null);
      void refresh();
    },
  });
  const reissue = useMutation({
    ...reissueInviteMutation(),
    onSuccess: (result, vars) => {
      const member = members?.find((m) => m.user.id === vars.path.user_id);
      setNotice(null);
      setInvite({ name: member ? userLabel(member.user) : "them", token: result.invite_token });
    },
  });

  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Role>("write");

  const row = (member: Member) => {
    const self = member.user.id === me?.id;
    return (
      <li
        key={member.user.id}
        className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-4 py-3"
      >
        <UserAvatar name={userLabel(member.user)} size="md" />
        <div className="min-w-0 flex-1 basis-40">
          <p className="truncate text-sm font-medium">
            {userLabel(member.user)}
            {self ? <span className="label-mono ml-2 text-muted-foreground">you</span> : null}
            {member.pending ? <span className="label-mono ml-2 text-signal">Invited</span> : null}
          </p>
          <p className="truncate text-xs text-muted-foreground">{member.user.email}</p>
        </div>
        {isAdmin ? (
          <RoleSelect
            label={`Role of ${userLabel(member.user)}`}
            value={member.role}
            onChange={(next) =>
              changeRole.mutate({
                path: { id: workspaceId, user_id: member.user.id },
                body: { role: next },
              })
            }
          />
        ) : (
          <span className="text-sm text-muted-foreground">{ROLE_NAME[member.role]}</span>
        )}
        {isAdmin ? (
          <span className="flex items-center gap-1">
            {member.pending ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() =>
                  reissue.mutate({ path: { id: workspaceId, user_id: member.user.id } })
                }
              >
                New invite link
              </Button>
            ) : null}
            {confirmRemove === member.user.id ? (
              <>
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  disabled={remove.isPending}
                  onClick={() =>
                    remove.mutate({ path: { id: workspaceId, user_id: member.user.id } })
                  }
                >
                  Remove
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setConfirmRemove(null)}
                >
                  Keep
                </Button>
              </>
            ) : (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="text-destructive"
                onClick={() => setConfirmRemove(member.user.id)}
              >
                Remove…
              </Button>
            )}
          </span>
        ) : null}
      </li>
    );
  };

  return (
    <div className="flex flex-col">
      <ul className="bg-card">{members?.map(row)}</ul>
      {changeRole.isError || remove.isError ? (
        <p role="alert" className="px-4 pt-3 text-sm text-destructive sm:px-6">
          That change isn't allowed. A workspace needs at least one admin.
        </p>
      ) : null}

      {invite ? (
        <div className="px-4 pt-4 sm:px-6">
          <InviteLinkBox
            name={invite.name}
            token={invite.token}
            onDismiss={() => setInvite(null)}
          />
        </div>
      ) : null}
      {notice ? (
        <p role="status" className="px-4 pt-4 text-sm text-muted-foreground sm:px-6">
          {notice}
        </p>
      ) : null}

      {isAdmin ? (
        <form
          className="grid gap-3 p-4 sm:grid-cols-[1fr_1fr_auto_auto] sm:items-end sm:px-6"
          onSubmit={(e) => {
            e.preventDefault();
            add.mutate({
              path: { id: workspaceId },
              body: { name: name.trim(), email: email.trim(), role },
            });
            setName("");
            setEmail("");
          }}
        >
          <Field label="Name (optional)" htmlFor="member-name">
            <Input id="member-name" value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="Email" htmlFor="member-email">
            <Input
              id="member-email"
              type="email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
          <Field label="Role">
            <RoleSelect label="Role of new member" value={role} onChange={setRole} />
          </Field>
          <Button type="submit" disabled={add.isPending}>
            Add member
          </Button>
          {add.isError ? (
            <p role="alert" className="text-sm text-destructive sm:col-span-4">
              Couldn't add them. They may already be a member.
            </p>
          ) : null}
        </form>
      ) : null}
    </div>
  );
}
