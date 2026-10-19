import { createFileRoute } from "@tanstack/react-router";

import { WikiPage } from "../components/wiki/WikiPage";

export const Route = createFileRoute("/w/$workspaceId/wiki/$pageId")({
  component: function WikiPageRoute() {
    const { workspaceId, pageId } = Route.useParams();
    return <WikiPage workspaceId={workspaceId} pageId={pageId} />;
  },
});
