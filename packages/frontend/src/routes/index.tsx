import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";
import { useMutation } from "@tanstack/react-query";
import { createWorkspaceMutation, listWorkspacesOptions } from "woodmouse-client/react-query";

import { InlineAdd } from "../components/common/InlineAdd";
import { requireLogin } from "../lib/auth";

export const Route = createFileRoute("/")({
  beforeLoad: async ({ context }) => {
    await requireLogin(context.queryClient);
    const workspaces = await context.queryClient.ensureQueryData(listWorkspacesOptions());
    if (workspaces.length > 0) {
      throw redirect({ to: "/w/$workspaceId", params: { workspaceId: workspaces[0].id } });
    }
  },
  component: FirstWorkspace,
});

/** Only reached when the user belongs to no workspace yet. */
function FirstWorkspace() {
  const navigate = useNavigate();
  const create = useMutation({
    ...createWorkspaceMutation(),
    onSuccess: async (workspace) => {
      void navigate({ to: "/w/$workspaceId", params: { workspaceId: workspace.id } });
    },
  });

  return (
    <main className="mx-auto flex max-w-md flex-col gap-4 px-6 py-20">
      <h1 className="font-display text-3xl font-bold">Start your first workspace</h1>
      <p className="text-sm text-muted-foreground">
        A workspace holds your projects and the people who work on them.
      </p>
      <InlineAdd
        label="Create workspace"
        placeholder="Workspace name"
        onSubmit={(name) => create.mutate({ body: { name } })}
      />
      {create.isError ? (
        <p className="text-sm text-destructive">Couldn't create the workspace. Try again.</p>
      ) : null}
    </main>
  );
}
