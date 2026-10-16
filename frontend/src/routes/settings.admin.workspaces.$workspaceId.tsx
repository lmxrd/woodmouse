import { createFileRoute, Link } from "@tanstack/react-router";

import {
  WorkspaceSettings,
  parseSettingsTab,
  type WorkspaceSettingsTab,
} from "../components/settings/WorkspaceSettings";

interface Search {
  tab?: WorkspaceSettingsTab;
}

export const Route = createFileRoute("/settings/admin/workspaces/$workspaceId")({
  validateSearch: (raw: Record<string, unknown>): Search => ({ tab: parseSettingsTab(raw.tab) }),
  component: AdminWorkspace,
});

/** A workspace's own settings, opened from the site admin. */
function AdminWorkspace() {
  const { workspaceId } = Route.useParams();
  const { tab } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <div className="-mx-4 -my-8 sm:-mx-6">
      <div className="px-4 pt-6 sm:px-6">
        <Link
          to="/settings/admin/workspaces"
          className="label-mono text-muted-foreground hover:text-foreground"
        >
          ← All workspaces
        </Link>
      </div>
      <WorkspaceSettings
        workspaceId={workspaceId}
        tab={tab}
        heading="Site admin · Workspace"
        onTab={(next) => void navigate({ search: { tab: next }, replace: true })}
      />
    </div>
  );
}
