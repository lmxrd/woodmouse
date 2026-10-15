import { createFileRoute, Outlet } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { getProjectOptions } from "woodmouse-client/react-query";

import { Board } from "../components/Board";

export const Route = createFileRoute("/w/$workspaceId/p/$projectId")({
  component: ProjectPage,
});

function ProjectPage() {
  const { workspaceId, projectId } = Route.useParams();
  const { data: project } = useQuery(getProjectOptions({ path: { id: projectId } }));

  return (
    <>
      <header className="flex items-end justify-between gap-4 border-b border-border px-6 pt-5 pb-4">
        <div>
          <p className="label-mono text-muted-foreground">Project</p>
          <h1 className="font-display text-2xl font-bold">{project?.name ?? " "}</h1>
        </div>
      </header>
      <Board projectId={projectId} workspaceId={workspaceId} />
      <Outlet />
    </>
  );
}
