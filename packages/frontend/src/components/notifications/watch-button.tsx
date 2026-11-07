import { useMutation, useQuery } from "@tanstack/react-query";
import {
  getPageWatchOptions,
  getTaskWatchOptions,
  setPageWatchMutation,
  setTaskWatchMutation,
} from "woodmouse-sdk/react-query";

import { Button } from "../ui/button";

/** Whether the signed-in person watches a task or page, and a way to change it, for both kinds. */
export function useWatch(kind: "task" | "page", id: string) {
  const taskQuery = useQuery({
    ...getTaskWatchOptions({ path: { id } }),
    enabled: kind === "task",
  });
  const pageQuery = useQuery({
    ...getPageWatchOptions({ path: { id } }),
    enabled: kind === "page",
  });
  const setTask = useMutation(setTaskWatchMutation());
  const setPage = useMutation(setPageWatchMutation());
  const watching = (kind === "task" ? taskQuery.data : pageQuery.data)?.watching ?? false;
  return {
    watching,
    toggle: () =>
      kind === "task"
        ? setTask.mutate({ path: { id }, body: { watching: !watching } })
        : setPage.mutate({ path: { id }, body: { watching: !watching } }),
  };
}

/** "Watch" / "Watching": you are notified of changes to what you watch. */
export function WatchButton({ kind, id }: { kind: "task" | "page"; id: string }) {
  const { watching, toggle } = useWatch(kind, id);
  return (
    <Button
      type="button"
      variant="ghost"
      size="xs"
      aria-pressed={watching}
      title={watching ? "You are notified of changes. Click to stop." : "Get notified of changes"}
      className="-mb-3 self-start font-normal text-muted-foreground"
      onClick={toggle}
    >
      {watching ? "✓ Watching" : "Watch"}
    </Button>
  );
}
