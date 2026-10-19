import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { AdminUser, Role } from "woodmouse-client";
import {
  adminCreateUserMutation,
  adminListUsersOptions,
  adminListWorkspacesOptions,
  adminReissueInviteMutation,
  adminUpdateUserMutation,
} from "woodmouse-client/react-query";

import { userLabel } from "../../lib/user";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Field } from "../common/Field";
import { ResponsiveDialog } from "../common/ResponsiveDialog";
import { UserAvatar } from "../common/UserAvatar";
import { InviteLinkBox } from "../settings/InviteLinkBox";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { RoleSelect as MembershipRole, useSetMembership } from "./memberships";

const NONE = "none";

/** Every account. Click one to edit it; "New user" makes an account and a one-time invite link. */
export function UsersTab({ meId }: { meId: string }) {
  const { data: users } = useQuery(adminListUsersOptions());
  const [editing, setEditing] = useState<AdminUser | "new" | null>(null);
  // Keep the open dialog in step with the list after a change.
  const current = editing && editing !== "new" ? users?.find((u) => u.id === editing.id) : editing;

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-display text-xl font-bold">Users</h2>
        <Button onClick={() => setEditing("new")}>New user</Button>
      </div>
      <div className="flex flex-col border border-border bg-card">
        {users?.map((u) => (
          <button
            key={u.id}
            type="button"
            onClick={() => setEditing(u)}
            className="flex items-center gap-3 border-b border-border px-4 py-3 text-left last:border-b-0 hover:bg-accent"
          >
            <UserAvatar name={userLabel(u)} size="md" />
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium">{userLabel(u)}</span>
              <span className="block truncate text-sm text-muted-foreground">{u.email}</span>
            </span>
            <span className="label-mono text-muted-foreground">
              {[
                u.super_admin ? "super admin" : null,
                u.pending ? "invited" : null,
                u.global_role ? `${u.global_role} everywhere` : null,
                u.memberships.length > 0
                  ? `${u.memberships.length} ${u.memberships.length === 1 ? "workspace" : "workspaces"}`
                  : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </button>
        ))}
      </div>
      <ResponsiveDialog
        open={editing !== null}
        onOpenChange={(open) => !open && setEditing(null)}
        title="User"
      >
        {current === "new" ? (
          <NewUser onDone={() => setEditing(null)} />
        ) : current ? (
          <EditUser
            key={current.id}
            user={current}
            locked={current.super_admin || current.id === meId}
          />
        ) : null}
      </ResponsiveDialog>
    </section>
  );
}

function NewUser({ onDone }: { onDone: () => void }) {
  const [role, setRole] = useState<string>(NONE);
  const { data: workspaces } = useQuery(adminListWorkspacesOptions());
  // Workspace id -> the role to add them with.
  const [picked, setPicked] = useState<Record<string, Role>>({});
  const create = useMutation({
    ...adminCreateUserMutation(),
    meta: { inline: true },
  });
  const created = create.data;

  return (
    <div className="flex flex-col gap-4 p-5">
      <h2 className="font-display text-xl font-bold">New user</h2>
      {created ? (
        <>
          <InviteLinkBox
            name={userLabel(created.user)}
            token={created.invite_token}
            onDismiss={onDone}
          />
        </>
      ) : (
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            const data = new FormData(e.currentTarget);
            create.mutate({
              body: {
                name: String(data.get("name") ?? ""),
                email: String(data.get("email") ?? ""),
                global_role: role === NONE ? null : (role as Role),
                memberships: Object.entries(picked).map(([workspace_id, r]) => ({
                  workspace_id,
                  role: r,
                })),
              },
            });
          }}
        >
          <Field label="Display name (optional)" htmlFor="new-user-name">
            <Input id="new-user-name" name="name" />
          </Field>
          <Field label="Email" htmlFor="new-user-email">
            <Input id="new-user-email" name="email" type="email" required />
          </Field>
          <RoleSelect value={role} onChange={setRole} />
          <Field label="Add to workspaces">
            <div className="flex flex-col border border-border">
              {workspaces
                ?.filter((w) => !w.deleted_at)
                .map((w) => (
                  <div
                    key={w.id}
                    className="flex items-center justify-between gap-3 border-b border-border px-3 py-1.5 last:border-b-0"
                  >
                    <span className="truncate text-sm">{w.name}</span>
                    <MembershipRole
                      label={`Role in ${w.name}`}
                      value={picked[w.id] ?? null}
                      onChange={(r) =>
                        setPicked((p) => {
                          const next = { ...p };
                          if (r) next[w.id] = r;
                          else delete next[w.id];
                          return next;
                        })
                      }
                    />
                  </div>
                ))}
            </div>
          </Field>
          {create.isError ? (
            <p role="alert" className="text-sm text-destructive">
              Couldn't create the account. The email may already be in use.
            </p>
          ) : null}
          <div className="flex gap-2">
            <Button type="submit" disabled={create.isPending}>
              Create and get invite link
            </Button>
            <Button type="button" variant="ghost" onClick={onDone}>
              Cancel
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

function RoleSelect({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return (
    <Field label="Access to every workspace">
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger aria-label="Global role" className="w-52">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NONE}>No global role</SelectItem>
          <SelectItem value="read">Read everywhere</SelectItem>
          <SelectItem value="write">Write everywhere</SelectItem>
          <SelectItem value="admin">Admin everywhere</SelectItem>
        </SelectContent>
      </Select>
    </Field>
  );
}

/** The workspaces a person belongs to, with their role in each, and a row to add another. */
function Memberships({ user }: { user: AdminUser }) {
  const { data: workspaces } = useQuery(adminListWorkspacesOptions());
  const { set, pending } = useSetMembership();
  const [workspaceId, setWorkspaceId] = useState<string>("");
  const [role, setRole] = useState<Role>("write");
  const joined = new Set(user.memberships.map((m) => m.workspace_id));
  const others = workspaces?.filter((w) => !w.deleted_at && !joined.has(w.id)) ?? [];

  return (
    <Field label="Workspaces">
      <div className="flex flex-col border border-border">
        {user.memberships.length === 0 ? (
          <p className="px-3 py-2 text-sm text-muted-foreground">Not in any workspace.</p>
        ) : null}
        {user.memberships.map((m) => (
          <div
            key={m.workspace_id}
            className="flex items-center justify-between gap-3 border-b border-border px-3 py-1.5"
          >
            <span className="truncate text-sm">{m.workspace_name}</span>
            <MembershipRole
              label={`Role in ${m.workspace_name}`}
              noneLabel="Remove"
              value={m.role}
              onChange={(r) => set(user, m.workspace_id, r)}
            />
          </div>
        ))}
        {others.length > 0 ? (
          <div className="flex items-center gap-2 px-3 py-1.5">
            <Select value={workspaceId} onValueChange={setWorkspaceId}>
              <SelectTrigger size="sm" aria-label="Add to workspace" className="min-w-0 flex-1">
                <SelectValue placeholder="Add to workspace…" />
              </SelectTrigger>
              <SelectContent>
                {others.map((w) => (
                  <SelectItem key={w.id} value={w.id}>
                    {w.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={role} onValueChange={(r) => setRole(r as Role)}>
              <SelectTrigger size="sm" aria-label="Role" className="w-24">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="read">Read</SelectItem>
                <SelectItem value="write">Write</SelectItem>
                <SelectItem value="admin">Admin</SelectItem>
              </SelectContent>
            </Select>
            <Button
              size="sm"
              disabled={!workspaceId || pending}
              onClick={() => {
                set(user, workspaceId, role);
                setWorkspaceId("");
              }}
            >
              Add
            </Button>
          </div>
        ) : null}
      </div>
    </Field>
  );
}

function EditUser({ user, locked }: { user: AdminUser; locked: boolean }) {
  const [invite, setInvite] = useState<string | null>(null);
  const [disabling, setDisabling] = useState(false);
  const update = useMutation({
    ...adminUpdateUserMutation(),
    meta: { inline: true },
  });
  const reinvite = useMutation({
    ...adminReissueInviteMutation(),
    onSuccess: (data) => setInvite(data.invite_token),
  });

  return (
    <div className="flex flex-col gap-4 p-5">
      <h2 className="font-display text-xl font-bold">{userLabel(user)}</h2>
      <form
        key={user.name + user.email}
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          update.mutate({
            path: { id: user.id },
            body: { name: String(data.get("name") ?? ""), email: String(data.get("email") ?? "") },
          });
        }}
      >
        <Field label="Display name" htmlFor="edit-user-name">
          <Input id="edit-user-name" name="name" defaultValue={user.name} disabled={locked} />
        </Field>
        <Field label="Email" htmlFor="edit-user-email">
          <Input
            id="edit-user-email"
            name="email"
            type="email"
            required
            defaultValue={user.email}
            disabled={locked}
          />
        </Field>
        {locked ? null : (
          <div>
            <Button type="submit" disabled={update.isPending}>
              Save
            </Button>
            {update.isError ? (
              <span role="alert" className="ml-3 text-sm text-destructive">
                Couldn't save. That email may already be in use.
              </span>
            ) : null}
          </div>
        )}
      </form>
      {locked ? (
        <p className="text-sm text-muted-foreground">
          {user.super_admin ? "The super admin" : "Your own account"} can't be changed here.
        </p>
      ) : (
        <>
          <RoleSelect
            value={user.global_role ?? NONE}
            onChange={(role) =>
              update.mutate({
                path: { id: user.id },
                body: { global_role: role === NONE ? null : (role as Role) },
              })
            }
          />
          <Memberships user={user} />
          {user.pending ? (
            <div className="flex flex-col gap-2">
              <p className="text-sm text-muted-foreground">They have not set a password yet.</p>
              {invite ? (
                <InviteLinkBox
                  name={userLabel(user)}
                  token={invite}
                  onDismiss={() => setInvite(null)}
                />
              ) : (
                <div>
                  <Button
                    variant="outline"
                    onClick={() => reinvite.mutate({ path: { id: user.id } })}
                  >
                    New invite link
                  </Button>
                </div>
              )}
            </div>
          ) : null}
          <div>
            {user.active ? (
              <Button variant="outline" onClick={() => setDisabling(true)}>
                Disable account
              </Button>
            ) : (
              <Button
                onClick={() => update.mutate({ path: { id: user.id }, body: { active: true } })}
              >
                Enable account
              </Button>
            )}
          </div>
        </>
      )}
      <ConfirmDialog
        open={disabling}
        onOpenChange={setDisabling}
        title={`Disable ${userLabel(user)}?`}
        description="They are signed out and cannot sign in. Everything they made stays, and enabling the account brings their access back."
        confirmLabel="Disable"
        onConfirm={() => update.mutate({ path: { id: user.id }, body: { active: false } })}
      />
    </div>
  );
}
