import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import type { ActivityView } from "woodmouse-sdk";
import { listActivityOptions, listProjectsOptions } from "woodmouse-sdk/react-query";

import { dayKey } from "../../lib/format";
import { useFormatters } from "../../lib/useFormatters";
import { ActivityItem } from "./ActivityItem";
import { FilterSelect } from "../common/FilterSelect";
import { Button } from "../ui/button";

const route = getRouteApi("/w/$workspaceId/activity");
const PAGE = 50;

/** The workspace's activity, newest first and grouped by day; narrowed to one project via the URL. */
export function ActivityPage({ workspaceId }: { workspaceId: string }) {
  const { project } = route.useSearch();
  const navigate = route.useNavigate();
  const { dateOf, dayOf, prefs } = useFormatters();
  const [limit, setLimit] = useState(PAGE);
  const [now] = useState(() => Date.now());
  const { data: projects } = useQuery(listProjectsOptions({ path: { workspace_id: workspaceId } }));
  const { data: entries } = useQuery(
    listActivityOptions({ path: { id: workspaceId }, query: { project_id: project, limit } }),
  );

  const dayLabel = (entry: ActivityView) => {
    const day = dayOf(entry.created_at);
    if (day === dayKey(now / 1000, prefs)) return "Today";
    if (day === dayKey(now / 1000 - 86_400, prefs)) return "Yesterday";
    return dateOf(entry.created_at);
  };

  const groups: { label: string; entries: ActivityView[] }[] = [];
  for (const entry of entries ?? []) {
    const label = dayLabel(entry);
    const last = groups[groups.length - 1];
    if (last?.label === label) last.entries.push(entry);
    else groups.push({ label, entries: [entry] });
  }

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Workspace</p>
          <h1 className="font-display text-2xl font-bold">Activity</h1>
        </div>
        <FilterSelect
          label="Project"
          anyLabel="All projects"
          value={project}
          options={(projects ?? []).map((p) => ({ value: p.id, label: p.name }))}
          onChange={(next) => void navigate({ search: { project: next }, replace: true })}
        />
      </header>
      {entries && entries.length === 0 ? (
        <p className="p-6 text-sm text-muted-foreground">Nothing has happened yet.</p>
      ) : null}
      {groups.map((group) => (
        <section key={group.label} aria-label={group.label}>
          <h2 className="label-mono border-b border-border px-4 py-2 text-muted-foreground sm:px-6">
            {group.label}
          </h2>
          {group.entries.map((entry) => (
            <ActivityItem key={entry.id} entry={entry} workspaceId={workspaceId} />
          ))}
        </section>
      ))}
      {entries && entries.length >= limit ? (
        <div className="p-4 sm:px-6">
          <Button type="button" variant="outline" onClick={() => setLimit(limit + PAGE)}>
            Load older
          </Button>
        </div>
      ) : null}
    </>
  );
}
