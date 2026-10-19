import { useState, type CSSProperties, type ReactNode } from "react";
import {
  DndContext,
  PointerSensor,
  TouchSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
} from "@dnd-kit/core";
import { Link } from "@tanstack/react-router";
import type { PageSummary } from "woodmouse-sdk";

import {
  ancestorIds,
  buildPageTree,
  dropChange,
  subtreeIds,
  zoneAt,
  type DropZone,
  type PageNode,
} from "../../lib/pageTree";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { PageMenu } from "./PageMenu";
import { usePageActions } from "./usePageActions";

/** The pointer's vertical position during a drag (the press position plus how far it has moved). */
function pointerY(event: DragMoveEvent | DragEndEvent): number | null {
  const start = event.activatorEvent as PointerEvent | TouchEvent;
  const y = "clientY" in start ? start.clientY : start.touches?.[0]?.clientY;
  return y === undefined ? null : y + event.delta.y;
}

/** One row: draggable by pressing anywhere on it, and a drop target that shows where it would land. */
function RowShell({
  id,
  draggable,
  zone,
  active,
  className,
  style,
  children,
}: {
  id: string;
  draggable: boolean;
  zone: DropZone | null;
  active: boolean;
  className: string;
  style: CSSProperties;
  children: ReactNode;
}) {
  const drag = useDraggable({ id, disabled: !draggable });
  const drop = useDroppable({ id, disabled: !draggable });
  const t = drag.transform;
  return (
    <div
      ref={(node) => {
        drag.setNodeRef(node);
        drop.setNodeRef(node);
      }}
      {...drag.listeners}
      className={cn(
        className,
        drag.isDragging && "relative z-10 opacity-70 shadow-md",
        !active && zone === "before" && "shadow-[inset_0_2px_0_0_var(--foreground)]",
        !active && zone === "after" && "shadow-[inset_0_-2px_0_0_var(--foreground)]",
        !active && zone === "into" && "outline outline-2 -outline-offset-2 outline-ring",
      )}
      style={{
        ...style,
        ...(t ? { transform: `translate3d(${t.x}px, ${t.y}px, 0)` } : {}),
      }}
    >
      {children}
    </div>
  );
}

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
  const { createPage, movePage } = usePageActions(workspaceId);
  // Pages the user opened or closed by hand; the rest are open when everything is (`openAll`) or when
  // they lie on the path to the open page.
  const [chosen, setChosen] = useState<Map<string, boolean>>(new Map());
  const onPath = new Set(activeId ? ancestorIds(pages, activeId) : []);
  const isOpen = (id: string) => chosen.get(id) ?? (openAll || onPath.has(id));
  const setOpen = (id: string, open: boolean) => setChosen((prev) => new Map(prev).set(id, open));

  // Drag a page onto the top or bottom edge of another to place it beside it, or onto the middle to
  // nest it. The menu's Move actions do the same without dragging (keyboard, and an alternative).
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 250, tolerance: 8 } }),
  );
  const [drop, setDrop] = useState<{ id: string; zone: DropZone } | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const blocked = dragging ? subtreeIds(pages, dragging) : new Set<string>();
  const hover = (event: DragMoveEvent) => {
    const y = pointerY(event);
    const over = event.over;
    if (!over || y === null || blocked.has(String(over.id))) return setDrop(null);
    setDrop({ id: String(over.id), zone: zoneAt(y, over.rect.top, over.rect.height) });
  };
  const finish = (event: DragEndEvent) => {
    const y = pointerY(event);
    setDragging(null);
    setDrop(null);
    if (!event.over || y === null) return;
    const target = String(event.over.id);
    const zone = zoneAt(y, event.over.rect.top, event.over.rect.height);
    const change = dropChange(pages, String(event.active.id), target, zone);
    if (!change) return;
    movePage(String(event.active.id), change);
    // Show the page where it landed when nesting into a collapsed page.
    if (zone === "into") setOpen(target, true);
  };

  const row = (node: PageNode) => {
    const { page } = node;
    const hasChildren = node.children.length > 0;
    const open = hasChildren && isOpen(page.id);
    const active = page.id === activeId;
    return (
      <li key={page.id}>
        <RowShell
          id={page.id}
          draggable={canWrite}
          zone={drop?.id === page.id ? drop.zone : null}
          active={dragging === page.id}
          className={cn("group flex items-center", active && "bg-accent")}
          style={{ paddingLeft: `${node.depth * 0.9}rem` }}
        >
          <button
            type="button"
            aria-label={open ? `Collapse ${page.title}` : `Expand ${page.title}`}
            aria-expanded={hasChildren ? open : undefined}
            disabled={!hasChildren}
            onClick={() => setOpen(page.id, !open)}
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
        </RowShell>
        {open && node.depth + 1 < maxDepth ? <ul>{node.children.map(row)}</ul> : null}
      </li>
    );
  };

  const tree = buildPageTree(pages);
  if (tree.length === 0) return null;
  return (
    <DndContext
      sensors={sensors}
      onDragStart={(event) => setDragging(String(event.active.id))}
      onDragMove={hover}
      onDragEnd={finish}
      onDragCancel={() => {
        setDragging(null);
        setDrop(null);
      }}
    >
      <ul>{tree.map(row)}</ul>
    </DndContext>
  );
}
