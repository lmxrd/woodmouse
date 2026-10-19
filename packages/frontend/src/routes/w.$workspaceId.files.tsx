import { createFileRoute } from "@tanstack/react-router";

import { FilesPage } from "../components/files/FilesPage";

interface FilesSearch {
  q?: string;
  unused?: boolean;
}

export const Route = createFileRoute("/w/$workspaceId/files")({
  validateSearch: (raw: Record<string, unknown>): FilesSearch => ({
    q: typeof raw.q === "string" && raw.q ? raw.q : undefined,
    unused: raw.unused === true || raw.unused === "true" ? true : undefined,
  }),
  component: FilesRoute,
});

function FilesRoute() {
  const { workspaceId } = Route.useParams();
  return <FilesPage workspaceId={workspaceId} />;
}
