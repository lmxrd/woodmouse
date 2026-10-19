import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import type { NotificationView } from "woodmouse-sdk";
import {
  listNotificationsOptions,
  markAllNotificationsReadMutation,
  markNotificationReadMutation,
  unreadNotificationCountOptions,
} from "woodmouse-sdk/react-query";

import { describeNotification, notificationQuote } from "../../lib/notifications";
import { useFormatters } from "../../lib/useFormatters";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** The notifications button with its unread count; opens the list. Polls the count once a minute. */
export function NotificationBell({ onNavigate }: { onNavigate?: () => void }) {
  const navigate = useNavigate();
  const { when } = useFormatters();
  const [open, setOpen] = useState(false);
  const { data: unread } = useQuery({
    ...unreadNotificationCountOptions(),
    refetchInterval: 60_000,
  });
  const { data: items } = useQuery({
    ...listNotificationsOptions({ query: { limit: 30 } }),
    enabled: open,
  });
  const markRead = useMutation(markNotificationReadMutation());
  const markAll = useMutation(markAllNotificationsReadMutation());
  const count = unread?.count ?? 0;

  const openItem = (n: NotificationView) => {
    if (!n.read) markRead.mutate({ path: { id: n.id } });
    setOpen(false);
    onNavigate?.();
    if (n.target_type === "page" && n.target_id) {
      void navigate({
        to: "/w/$workspaceId/wiki/$pageId",
        params: { workspaceId: n.workspace_id, pageId: n.target_id },
      });
    } else if (n.target_id) {
      void navigate({
        to: "/w/$workspaceId/t/$taskId",
        params: { workspaceId: n.workspace_id, taskId: n.target_id },
      });
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="flex w-full items-center justify-between px-2 py-1.5 text-left text-sm text-muted-foreground hover:text-foreground"
          aria-label={count > 0 ? `Notifications, ${count} unread` : "Notifications"}
        >
          <span>Notifications</span>
          {count > 0 ? (
            <span className="min-w-5 bg-signal px-1.5 text-center font-mono text-[11px] text-white">
              {count > 99 ? "99+" : count}
            </span>
          ) : null}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[min(24rem,calc(100vw-2rem))] p-0">
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <h2 className="label-mono text-muted-foreground">Notifications</h2>
          <div className="flex items-center gap-1">
            {count > 0 ? (
              <Button variant="ghost" size="xs" onClick={() => markAll.mutate({})}>
                Mark all read
              </Button>
            ) : null}
            <Button variant="ghost" size="xs" asChild>
              <Link to="/settings/notifications" onClick={() => setOpen(false)}>
                Settings
              </Link>
            </Button>
          </div>
        </div>
        <ul className="max-h-96 overflow-y-auto">
          {items?.length === 0 ? (
            <li className="px-3 py-6 text-center text-sm text-muted-foreground">
              Nothing yet. Watch a task or page to hear about changes.
            </li>
          ) : null}
          {items?.map((n) => (
            <li key={n.id}>
              <button
                type="button"
                onClick={() => openItem(n)}
                className={cn(
                  "flex w-full flex-col gap-0.5 border-b border-border px-3 py-2 text-left text-sm last:border-b-0 hover:bg-accent",
                  !n.read && "bg-accent/40",
                )}
              >
                <span className={cn(!n.read && "font-medium")}>{describeNotification(n)}</span>
                {notificationQuote(n) ? (
                  <span className="truncate text-muted-foreground">“{notificationQuote(n)}”</span>
                ) : null}
                <span className="label-mono text-muted-foreground">{when(n.created_at)}</span>
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
