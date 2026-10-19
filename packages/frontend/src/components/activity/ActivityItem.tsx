import { Link } from "@tanstack/react-router";
import type { ActivityView } from "woodmouse-sdk";

import { actorLabel, describeActivity } from "../../lib/activity";
import { useFormatters } from "../../lib/useFormatters";
import { UserAvatar } from "../common/UserAvatar";

/** One line of an activity feed: who, what, and when. */
export function ActivityItem({ entry, workspaceId }: { entry: ActivityView; workspaceId: string }) {
  const { date, when } = useFormatters();
  const { text, quote } = describeActivity(entry, date);
  return (
    <div className="flex gap-3 border-b border-border bg-card px-4 py-2.5 sm:px-6">
      <UserAvatar name={actorLabel(entry)} size="md" className="mt-0.5" />
      <div className="min-w-0 flex-1 text-sm">
        <p>
          <b className="font-medium">{actorLabel(entry)}</b>{" "}
          {entry.task_id ? (
            <Link
              to="/w/$workspaceId/t/$taskId"
              params={{ workspaceId, taskId: entry.task_id }}
              className="hover:underline"
            >
              {text}
            </Link>
          ) : (
            text
          )}
        </p>
        {quote ? <p className="truncate text-muted-foreground">“{quote}”</p> : null}
      </div>
      <span className="label-mono shrink-0 text-muted-foreground">{when(entry.created_at)}</span>
    </div>
  );
}
