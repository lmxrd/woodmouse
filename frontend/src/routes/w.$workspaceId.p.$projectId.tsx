import { createFileRoute, Outlet } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { getProjectOptions } from "woodmouse-client/react-query";

import { Board } from "../components/Board";
import { ProjectViewMenu } from "../components/ProjectViewMenu";
import { TaskListView } from "../components/TaskListView";
import { validateProjectSearch } from "../lib/projectSearch";

export const Route = createFileRoute("/w/$workspaceId/p/$projectId")({
  validateSearch: validateProjectSearch,
  component: ProjectPage,
});

function ProjectPage() {
  const { workspaceId, projectId } = Route.useParams();
  const { view = "board" } = Route.useSearch();
  const navigate = Route.useNavigate();
  const { data: project } = useQuery(getProjectOptions({ path: { id: projectId } }));

  return (
    <>
      <header className="flex items-end justify-between gap-4 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Project</p>
          <h1 className="font-display text-2xl font-bold">{project?.name ?? "\u00a0"}</h1>
        </div>
        <ProjectViewMenu
          value={view}
          onChange={(next) => void navigate({ search: (prev) => ({ ...prev, view: next }) })}
        />
      </header>
      {view === "list" ? (
        <TaskListView workspaceId={workspaceId} projectId={projectId} />
      ) : (
        <Board projectId={projectId} workspaceId={workspaceId} />
      )}
      <Outlet />
    </>
  );
}
