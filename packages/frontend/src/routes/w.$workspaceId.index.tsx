import { createFileRoute, redirect } from "@tanstack/react-router";
import { listProjectsOptions } from "woodmouse-sdk/react-query";

export const Route = createFileRoute("/w/$workspaceId/")({
  beforeLoad: async ({ context, params }) => {
    const projects = await context.queryClient.ensureQueryData(
      listProjectsOptions({ path: { workspace_id: params.workspaceId } }),
    );
    if (projects.length > 0) {
      throw redirect({
        to: "/w/$workspaceId/p/$projectId",
        params: { workspaceId: params.workspaceId, projectId: projects[0].id },
      });
    }
  },
  component: NoProjects,
});

function NoProjects() {
  return (
    <div className="flex flex-col gap-2 px-6 py-8">
      <h1 className="font-display text-2xl font-bold">Start your first project</h1>
      <p className="text-sm text-muted-foreground">
        Projects hold lists of tasks. Create one from the sidebar.
      </p>
    </div>
  );
}
