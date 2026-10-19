import { createFileRoute } from "@tanstack/react-router";

import { ActivityPage } from "../components/activity/ActivityPage";

interface ActivitySearch {
  project?: string;
}

export const Route = createFileRoute("/w/$workspaceId/activity")({
  validateSearch: (raw: Record<string, unknown>): ActivitySearch => ({
    project: typeof raw.project === "string" && raw.project ? raw.project : undefined,
  }),
  component: ActivityRoute,
});

function ActivityRoute() {
  const { workspaceId } = Route.useParams();
  return <ActivityPage workspaceId={workspaceId} />;
}
