import { createFileRoute } from "@tanstack/react-router";

import {
  WorkspaceSettings,
  parseSettingsTab,
  type WorkspaceSettingsTab,
} from "../components/settings/WorkspaceSettings";

interface SettingsSearch {
  tab?: WorkspaceSettingsTab;
}

export const Route = createFileRoute("/w/$workspaceId/settings")({
  validateSearch: (raw: Record<string, unknown>): SettingsSearch => ({
    tab: parseSettingsTab(raw.tab),
  }),
  component: SettingsPage,
});

function SettingsPage() {
  const { workspaceId } = Route.useParams();
  const { tab } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <WorkspaceSettings
      workspaceId={workspaceId}
      tab={tab}
      onTab={(next) => void navigate({ search: { tab: next } })}
    />
  );
}
