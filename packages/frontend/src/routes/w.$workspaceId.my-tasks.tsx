import { createFileRoute } from "@tanstack/react-router";

import { MyTasks } from "../components/task/my-tasks";
import { validateProjectSearch, type ProjectSearch } from "../lib/project-search";

type MyTasksSearch = Pick<ProjectSearch, "status"> & { project?: string };

export const Route = createFileRoute("/w/$workspaceId/my-tasks")({
  validateSearch: (raw: Record<string, unknown>): MyTasksSearch => ({
    status: validateProjectSearch(raw).status,
    project: typeof raw.project === "string" && raw.project ? raw.project : undefined,
  }),
  component: MyTasksPage,
});

function MyTasksPage() {
  const { workspaceId } = Route.useParams();
  return <MyTasks workspaceId={workspaceId} />;
}
