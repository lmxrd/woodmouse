import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import type { PageSummary } from "woodmouse-client";
import { listPagesOptions } from "woodmouse-client/react-query";

import { positionBetween } from "../../lib/position";
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
}: {
  page: PageSummary;
  workspaceId: string;
  /** Go back to the wiki overview after deleting (when the page itself is open). */
  leaveOnDelete?: boolean;
}) {
  const navigate = useNavigate();
  const { data: pages = [] } = useQuery(listPagesOptions({ path: { id: workspaceId } }));
  const { createPage, movePage, deletePage } = usePageActions(workspaceId);
  const [moving, setMoving] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const siblings = pages
    .filter((p) => (p.parent_id ?? null) === (page.parent_id ?? null))
    .sort((a, b) => a.position - b.position);
  const index = siblings.findIndex((p) => p.id === page.id);
  const others = siblings.filter((p) => p.id !== page.id);
  const reorder = (to: number) => movePage(page.id, { position: positionBetween(others, to) });
  const subpages = subtreeIds(pages, page.id).size - 1;

  return (
    <>
      <ActionMenu
        label={`Actions for ${page.title}`}
        items={[
          { label: "Add subpage", onSelect: () => createPage(page.id) },
          ...(index > 0 ? [{ label: "Move up", onSelect: () => reorder(index - 1) }] : []),
          ...(index >= 0 && index < siblings.length - 1
            ? [{ label: "Move down", onSelect: () => reorder(index + 1) }]
            : []),
          { label: "Move to…", onSelect: () => setMoving(true) },
          { label: "Delete…", destructive: true, onSelect: () => setDeleting(true) },
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
