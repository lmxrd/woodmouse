import { createFileRoute } from "@tanstack/react-router";

import { TrashPage } from "../components/TrashPage";

export const Route = createFileRoute("/w/$workspaceId/trash")({
  component: TrashRoute,
});

function TrashRoute() {
  const { workspaceId } = Route.useParams();
  return <TrashPage workspaceId={workspaceId} />;
}
