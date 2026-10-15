import { createFileRoute } from "@tanstack/react-router";

import { FilesPage } from "../components/files/FilesPage";

export const Route = createFileRoute("/w/$workspaceId/files")({
  component: FilesRoute,
});

function FilesRoute() {
  const { workspaceId } = Route.useParams();
  return <FilesPage workspaceId={workspaceId} />;
}
