import { createFileRoute } from "@tanstack/react-router";

import { WorkspacesTab } from "../components/admin/WorkspacesTab";

interface Search {
  deleted?: boolean;
  /** The access matrix instead of the list. */
  matrix?: true;
}

export const Route = createFileRoute("/settings/admin/workspaces/")({
  validateSearch: (raw: Record<string, unknown>): Search => ({
    deleted: raw.deleted === true || raw.deleted === "true" ? true : undefined,
    matrix: raw.matrix === true || raw.matrix === "true" ? true : undefined,
  }),
  component: Workspaces,
});

function Workspaces() {
  const { deleted, matrix } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <WorkspacesTab
      showDeleted={!!deleted}
      onShowDeleted={(show) =>
        void navigate({
          search: (prev) => ({ ...prev, deleted: show ? true : undefined }),
          replace: true,
        })
      }
      view={matrix ? "matrix" : "list"}
      onView={(next) =>
        void navigate({
          search: (prev) => ({ ...prev, matrix: next === "matrix" ? true : undefined }),
          replace: true,
        })
      }
    />
  );
}
