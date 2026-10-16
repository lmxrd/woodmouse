import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-client";
import { adminListUsersOptions, adminUpdateUserMutation } from "woodmouse-client/react-query";

import { userLabel } from "../../lib/user";
import { ConfirmDialog } from "../ConfirmDialog";
import { UserAvatar } from "../UserAvatar";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const NONE = "none";

/** Every account: its global role (access to every workspace) and enabling or disabling it. */
export function UsersTab({ meId }: { meId: string }) {
  const queryClient = useQueryClient();
  const { data: users } = useQuery(adminListUsersOptions());
  const update = useMutation({
    ...adminUpdateUserMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  const [disabling, setDisabling] = useState<PublicUser | null>(null);

  return (
    <section className="flex flex-col border border-border bg-card">
      {users?.map((u) => {
        const locked = u.super_admin || u.id === meId;
        return (
          <div
            key={u.id}
            className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-3 last:border-b-0"
          >
            <UserAvatar name={userLabel(u)} size="md" />
            <div className="min-w-0 flex-1">
              <p className="truncate font-medium">
                {userLabel(u)}
                {u.super_admin ? (
                  <span className="label-mono ml-2 text-muted-foreground">super admin</span>
                ) : null}
              </p>
              <p className="truncate text-sm text-muted-foreground">{u.email}</p>
            </div>
            {locked ? null : (
              <>
                <Select
                  value={u.global_role ?? NONE}
                  onValueChange={(role) =>
                    update.mutate({
                      path: { id: u.id },
                      body: {
                        global_role: role === NONE ? null : (role as PublicUser["global_role"]),
                      },
                    })
                  }
                >
                  <SelectTrigger aria-label={`Global role of ${userLabel(u)}`} className="w-44">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE}>No global role</SelectItem>
                    <SelectItem value="read">Read everywhere</SelectItem>
                    <SelectItem value="write">Write everywhere</SelectItem>
                    <SelectItem value="admin">Admin everywhere</SelectItem>
                  </SelectContent>
                </Select>
                {u.active ? (
                  <Button variant="outline" size="sm" onClick={() => setDisabling(u)}>
                    Disable
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    onClick={() => update.mutate({ path: { id: u.id }, body: { active: true } })}
                  >
                    Enable
                  </Button>
                )}
              </>
            )}
          </div>
        );
      })}
      <ConfirmDialog
        open={disabling !== null}
        onOpenChange={(open) => !open && setDisabling(null)}
        title={`Disable ${disabling ? userLabel(disabling) : ""}?`}
        description="They are signed out and cannot sign in. Everything they made stays, and enabling the account brings their access back."
        confirmLabel="Disable"
        onConfirm={() =>
          disabling && update.mutate({ path: { id: disabling.id }, body: { active: false } })
        }
      />
    </section>
  );
}
