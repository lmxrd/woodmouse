import { useQuery } from "@tanstack/react-query";
import { adminListUsersOptions, adminListWorkspacesOptions } from "woodmouse-sdk/react-query";

import { userLabel } from "../../lib/user";
import { RoleSelect, useSetMembership } from "./memberships";

/** Everyone's role in every workspace on one screen: people down the side, workspaces across the top. */
export function MembersMatrix() {
  const { data: users } = useQuery(adminListUsersOptions());
  const { data: workspaces } = useQuery(adminListWorkspacesOptions());
  const { set } = useSetMembership();
  const live = workspaces?.filter((w) => !w.deleted_at) ?? [];

  if (live.length === 0) {
    return (
      <p className="border border-dashed border-border px-4 py-6 text-muted-foreground">
        No workspaces.
      </p>
    );
  }
  return (
    <div className="overflow-x-auto border border-border bg-card">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border text-left">
            <th className="label-mono sticky left-0 bg-card px-3 py-2 font-normal text-muted-foreground">
              Person
            </th>
            {live.map((w) => (
              <th key={w.id} className="px-2 py-2 font-medium whitespace-nowrap">
                {w.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {users?.map((u) => (
            <tr key={u.id} className="border-b border-border last:border-b-0">
              <th className="sticky left-0 bg-card px-3 py-1.5 text-left font-normal whitespace-nowrap">
                <span className="block">{userLabel(u)}</span>
                {u.super_admin || u.global_role ? (
                  <span className="label-mono text-muted-foreground">
                    {u.super_admin ? "super admin" : `${u.global_role} everywhere`}
                  </span>
                ) : null}
              </th>
              {live.map((w) => (
                <td key={w.id} className="px-2 py-1.5">
                  {u.super_admin ? (
                    <span className="label-mono text-muted-foreground">admin</span>
                  ) : (
                    <RoleSelect
                      label={`${userLabel(u)} in ${w.name}`}
                      noneLabel="—"
                      className="w-24"
                      value={u.memberships.find((m) => m.workspace_id === w.id)?.role ?? null}
                      onChange={(role) => set(u, w.id, role)}
                    />
                  )}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
