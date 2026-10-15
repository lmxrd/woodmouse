import { useQuery } from "@tanstack/react-query";
import { listMembersOptions, meOptions } from "woodmouse-client/react-query";

/** The current user's standing in a workspace. Super admins count as admins everywhere. */
export function useWorkspaceRole(workspaceId: string) {
  const { data: me } = useQuery(meOptions());
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const role = members?.find((m) => m.user.id === me?.id)?.role;
  const isAdmin = !!me?.super_admin || role === "admin";
  return { isAdmin, canWrite: isAdmin || role === "write" };
}
