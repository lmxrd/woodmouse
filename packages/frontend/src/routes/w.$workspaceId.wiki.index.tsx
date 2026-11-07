import { createFileRoute } from "@tanstack/react-router";

import { WikiIndex } from "../components/wiki/wiki-index";

export const Route = createFileRoute("/w/$workspaceId/wiki/")({
  component: function WikiIndexRoute() {
    const { workspaceId } = Route.useParams();
    return <WikiIndex workspaceId={workspaceId} />;
  },
});
