import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import type { PageSummary } from "woodmouse-client";
import { listPagesOptions } from "woodmouse-client/react-query";

import { pageExportUrl } from "../../lib/api";
import { useWatch } from "../notifications/WatchButton";
import { afterIdAt } from "../../lib/position";
import { subtreeIds } from "../../lib/pageTree";
import { ActionMenu } from "../ActionMenu";
import { ConfirmDialog } from "../ConfirmDialog";
import { MovePageDialog } from "./MovePageDialog";
import { usePageActions } from "./usePageActions";

/** The "⋯" menu of a page: subpage, reorder, move and delete. */
export function PageMenu({
  page,
  workspaceId,
  leaveOnDelete = false,
  canWrite = true,
  onHistory,
}: {
  page: PageSummary;
  workspaceId: string;
  /** Go back to the wiki overview after deleting (when the page itself is open). */
  leaveOnDelete?: boolean;
  /** Adds "Version history" to the menu (only where the page's editor is open). */
  onHistory?: () => void;
  /** Without write access the menu only offers history and export. */
  canWrite?: boolean;
}) {
  const navigate = useNavigate();
  const { data: pages = [] } = useQuery(listPagesOptions({ path: { id: workspaceId } }));
  const { createPage, movePage, deletePage } = usePageActions(workspaceId);
  const [moving, setMoving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { watching, toggle: toggleWatch } = useWatch("page", page.id);

  const siblings = pages
    .filter((p) => (p.parent_id ?? null) === (page.parent_id ?? null))
    .sort((a, b) => a.position - b.position);
  const index = siblings.findIndex((p) => p.id === page.id);
  const others = siblings.filter((p) => p.id !== page.id);
  const reorder = (to: number) => movePage(page.id, { after_id: afterIdAt(others, to) });
  const subpages = subtreeIds(pages, page.id).size - 1;

  return (
    <>
      <ActionMenu
        label={`Actions for ${page.title}`}
        items={[
          ...(canWrite
            ? [
                { label: "Add subpage", onSelect: () => createPage(page.id) },
                ...(index > 0 ? [{ label: "Move up", onSelect: () => reorder(index - 1) }] : []),
                ...(index >= 0 && index < siblings.length - 1
                  ? [{ label: "Move down", onSelect: () => reorder(index + 1) }]
                  : []),
                { label: "Move to…", onSelect: () => setMoving(true) },
              ]
            : []),
          { label: watching ? "Stop watching" : "Watch page", onSelect: toggleWatch },
          ...(onHistory ? [{ label: "Version history", onSelect: onHistory }] : []),
          {
            label: "Export as Markdown",
            // The server answers with a file attachment, so this downloads without leaving the page.
            onSelect: () => window.location.assign(pageExportUrl(page.id)),
          },
          ...(canWrite
            ? [{ label: "Delete…", destructive: true, onSelect: () => setDeleting(true) }]
            : []),
        ]}
      />
      <MovePageDialog
        open={moving}
        onOpenChange={setMoving}
        page={page}
        pages={pages}
        onMove={(parentId) => movePage(page.id, { parent_id: parentId })}
      />
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={`Delete "${page.title}"?`}
        description={
          subpages > 0
            ? `This also deletes its ${subpages} ${subpages === 1 ? "subpage" : "subpages"}. You can restore them from the trash.`
            : "You can restore it from the trash."
        }
        confirmLabel="Delete page"
        onConfirm={() =>
          deletePage(
            page.id,
            leaveOnDelete
              ? () => void navigate({ to: "/w/$workspaceId/wiki", params: { workspaceId } })
              : undefined,
          )
        }
      />
    </>
  );
}
