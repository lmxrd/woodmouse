import { useState } from "react";
import { Link } from "@tanstack/react-router";
import type { PageSummary } from "woodmouse-client";

import { ancestorIds, buildPageTree, type PageNode } from "../../lib/pageTree";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { PageMenu } from "./PageMenu";
import { usePageActions } from "./usePageActions";

/**
 * The wiki's page tree. `maxDepth` cuts it off for the sidebar (deeper pages stay reachable from the
 * wiki overview); `openAll` starts everything expanded. The path to the open page is always open.
 */
export function PageTree({
  pages,
  workspaceId,
  activeId,
  canWrite,
  maxDepth = Infinity,
  openAll = false,
  onNavigate,
}: {
  pages: PageSummary[];
  workspaceId: string;
  activeId?: string;
  canWrite: boolean;
  maxDepth?: number;
  openAll?: boolean;
  onNavigate?: () => void;
}) {
  const { createPage } = usePageActions(workspaceId);
  // Pages the user flipped away from the default (open when on the path to the active page).
  const [flipped, setFlipped] = useState<Set<string>>(new Set());
  const onPath = new Set(activeId ? ancestorIds(pages, activeId) : []);
  const isOpen = (id: string) => (openAll || onPath.has(id)) !== flipped.has(id);
  const toggle = (id: string) =>
    setFlipped((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const row = (node: PageNode) => {
    const { page } = node;
    const hasChildren = node.children.length > 0;
    const open = hasChildren && isOpen(page.id);
    const active = page.id === activeId;
    return (
      <li key={page.id}>
        <div
          className={cn("group flex items-center", active && "bg-accent")}
          style={{ paddingLeft: `${node.depth * 0.9}rem` }}
        >
          <button
            type="button"
            aria-label={open ? `Collapse ${page.title}` : `Expand ${page.title}`}
            aria-expanded={hasChildren ? open : undefined}
            disabled={!hasChildren}
            onClick={() => toggle(page.id)}
            className="grid size-6 shrink-0 place-items-center text-xs text-muted-foreground disabled:opacity-0"
          >
            {open ? "▾" : "▸"}
          </button>
          <Link
            to="/w/$workspaceId/wiki/$pageId"
            params={{ workspaceId, pageId: page.id }}
            onClick={onNavigate}
            className={cn(
              "min-w-0 flex-1 truncate py-1 pr-1 text-sm",
              active
                ? "font-medium text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {page.title}
          </Link>
          {canWrite ? (
            <span className="flex shrink-0 opacity-100 md:opacity-0 md:group-focus-within:opacity-100 md:group-hover:opacity-100">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Add subpage to ${page.title}`}
                onClick={() => createPage(page.id)}
              >
                +
              </Button>
              <PageMenu page={page} workspaceId={workspaceId} />
            </span>
          ) : null}
        </div>
        {open && node.depth + 1 < maxDepth ? <ul>{node.children.map(row)}</ul> : null}
      </li>
    );
  };

  const tree = buildPageTree(pages);
  return tree.length === 0 ? null : <ul>{tree.map(row)}</ul>;
}
