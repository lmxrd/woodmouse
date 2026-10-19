import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { PageDetail } from "woodmouse-sdk";
import {
  getPageRevisionOptions,
  listPageRevisionsOptions,
  restorePageRevisionMutation,
} from "woodmouse-sdk/react-query";

import { HistoryPanel } from "../history/HistoryPanel";
import { BlockPreview } from "./BlockPreview";

function Preview({ revisionId, workspaceId }: { revisionId: string; workspaceId: string }) {
  const { data } = useQuery(getPageRevisionOptions({ path: { id: revisionId } }));
  if (!data) return <p className="text-muted-foreground">Loading…</p>;
  return (
    <div className="flex flex-col gap-3">
      <h3 className="font-display text-2xl font-bold">{data.title}</h3>
      <BlockPreview blocks={data.blocks} workspaceId={workspaceId} />
    </div>
  );
}

/** The version history of a page: browse earlier content and put it back. */
export function PageHistory({
  pageId,
  workspaceId,
  open,
  onOpenChange,
  canRestore,
  onRestored,
}: {
  pageId: string;
  workspaceId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  canRestore: boolean;
  /** Called with the page after a restore, so the editor can show the restored content. */
  onRestored: (page: PageDetail) => void;
}) {
  const queryClient = useQueryClient();
  const { data: revisions = [] } = useQuery({
    ...listPageRevisionsOptions({ path: { id: pageId } }),
    enabled: open,
  });
  const restore = useMutation({
    ...restorePageRevisionMutation(),
    onSuccess: (page) => {
      void queryClient.invalidateQueries();
      onOpenChange(false);
      onRestored(page);
      toast.success("Version restored. What was there before is kept in the history.");
    },
  });

  return (
    <HistoryPanel
      open={open}
      onOpenChange={onOpenChange}
      title="Version history"
      revisions={revisions}
      canRestore={canRestore}
      restoring={restore.isPending}
      onRestore={(id) => restore.mutate({ path: { id } })}
      renderPreview={(id) => <Preview revisionId={id} workspaceId={workspaceId} />}
    />
  );
}
