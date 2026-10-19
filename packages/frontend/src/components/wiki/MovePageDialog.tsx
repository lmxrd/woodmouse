import type { PageSummary } from "woodmouse-client";

import { buildPageTree, subtreeIds, type PageNode } from "../../lib/pageTree";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";

/** Pick the page (or the top level) to move a page under. A page cannot go below itself. */
export function MovePageDialog({
  open,
  onOpenChange,
  page,
  pages,
  onMove,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  page: PageSummary;
  pages: PageSummary[];
  onMove: (parentId: string | null) => void;
}) {
  const blocked = subtreeIds(pages, page.id);
  const pick = (parentId: string | null) => {
    onMove(parentId);
    onOpenChange(false);
  };
  const rows = (nodes: PageNode[]): React.ReactNode =>
    nodes
      .filter((n) => !blocked.has(n.page.id))
      .map((n) => (
        <li key={n.page.id}>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={n.page.id === page.parent_id}
            className="w-full justify-start font-normal"
            style={{ paddingLeft: `${0.5 + n.depth * 1.25}rem` }}
            onClick={() => pick(n.page.id)}
          >
            <span className="truncate">{n.page.title}</span>
          </Button>
          {n.children.length > 0 ? <ul>{rows(n.children)}</ul> : null}
        </li>
      ));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="rounded-none">
        <DialogHeader>
          <DialogTitle>Move "{page.title}" to…</DialogTitle>
          <DialogDescription>Choose the page it should go under.</DialogDescription>
        </DialogHeader>
        <ul className="max-h-80 overflow-y-auto">
          <li>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={!page.parent_id}
              className="w-full justify-start font-normal"
              onClick={() => pick(null)}
            >
              Top level
            </Button>
          </li>
          {rows(buildPageTree(pages))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}
