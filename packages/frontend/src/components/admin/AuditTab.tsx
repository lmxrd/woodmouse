import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { adminAuditLogOptions, adminRedactRevisionMutation } from "woodmouse-client/react-query";

import { userLabel } from "../../lib/user";
import { useFormatters } from "../../lib/useFormatters";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const ALL = "all";
const PAGE = 100;
const ACTIONS = [
  "login",
  "login_failed",
  "logout",
  "password_changed",
  "role_changed",
  "user_enabled",
  "user_disabled",
  "entity_deleted",
  "entity_restored",
  "task_edited",
  "page_edited",
  "comment_edited",
  "revision_redacted",
  "backup_created",
];

/** The whole audit log, newest first; the action filter is in the URL. */
export function AuditTab({
  action,
  onAction,
}: {
  action: string | undefined;
  onAction: (action: string | undefined) => void;
}) {
  const { dateTime } = useFormatters();
  const [limit, setLimit] = useState(PAGE);
  const [redacting, setRedacting] = useState<string | null>(null);
  const { data: entries } = useQuery(adminAuditLogOptions({ query: { action, limit } }));
  const redact = useMutation({
    ...adminRedactRevisionMutation(),
  });

  return (
    <section className="flex flex-col gap-3">
      <Select value={action ?? ALL} onValueChange={(a) => onAction(a === ALL ? undefined : a)}>
        <SelectTrigger aria-label="Action" className="w-56">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>All actions</SelectItem>
          {ACTIONS.map((a) => (
            <SelectItem key={a} value={a}>
              {a.replaceAll("_", " ")}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <div className="flex flex-col border border-border bg-card">
        {entries?.length === 0 ? (
          <p className="px-4 py-6 text-muted-foreground">Nothing recorded.</p>
        ) : null}
        {entries?.map((e) => {
          const revision = typeof e.data.revision_id === "string" ? e.data.revision_id : null;
          return (
            <div
              key={e.id}
              className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border px-4 py-2.5 text-sm last:border-b-0"
            >
              <span className="min-w-0 flex-1">
                <b className="font-medium">{e.actor ? userLabel(e.actor) : "Someone"}</b>{" "}
                {e.action.replaceAll("_", " ")}
                {e.target_type ? (
                  <span className="text-muted-foreground"> · {e.target_type}</span>
                ) : null}
              </span>
              {revision ? (
                <Button variant="ghost" size="xs" onClick={() => setRedacting(revision)}>
                  Redact content
                </Button>
              ) : null}
              <span className="label-mono text-muted-foreground">{dateTime(e.created_at)}</span>
            </div>
          );
        })}
      </div>
      {entries && entries.length >= limit && limit < 500 ? (
        <div>
          <Button variant="outline" onClick={() => setLimit((l) => l + PAGE)}>
            Load more
          </Button>
        </div>
      ) : null}
      <ConfirmDialog
        open={redacting !== null}
        onOpenChange={(open) => !open && setRedacting(null)}
        title="Redact this version?"
        description="Its content is removed for good. The version and this log entry stay."
        confirmLabel="Redact"
        onConfirm={() => redacting && redact.mutate({ path: { id: redacting } })}
      />
    </section>
  );
}
