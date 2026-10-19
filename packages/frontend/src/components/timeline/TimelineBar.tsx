import { useRef, useState } from "react";
import type { TaskResponse } from "woodmouse-sdk";

import { addDays } from "../../lib/calendar";
import type { Zoom } from "../../lib/projectSearch";
import { PX_PER_DAY, barBox, progressOf, type Span } from "../../lib/timeline";
import { cn } from "../../lib/utils";
import { isDone } from "../../lib/task";

type Mode = "move" | "start" | "end";

/** The span while a drag is in flight: moved as a whole, or one edge pulled (never past the other). */
export function previewSpan(span: Span, mode: Mode, deltaDays: number): Span {
  if (mode === "move")
    return { start: addDays(span.start, deltaDays), end: addDays(span.end, deltaDays) };
  if (mode === "start")
    return { start: Math.min(addDays(span.start, deltaDays), span.end), end: span.end };
  return { start: span.start, end: Math.max(addDays(span.end, deltaDays), span.start) };
}

/**
 * A task's bar (or a diamond when it is a single day). With a mouse it can be dragged to move and its
 * ends pulled to resize; on touch screens tapping opens the task. Derived parent bars are read-only.
 */
export function TimelineBar({
  task,
  span,
  derived,
  from,
  zoom,
  today,
  editable,
  onPreview,
  onCommit,
  onOpen,
}: {
  task: TaskResponse;
  span: Span;
  derived: boolean;
  from: number;
  zoom: Zoom;
  today: number;
  editable: boolean;
  /** Reports the span being previewed (for the arrows), or null when the drag ends. */
  onPreview: (taskId: string, span: Span | null) => void;
  onCommit: (task: TaskResponse, mode: Mode, deltaDays: number) => void;
  onOpen: () => void;
}) {
  const px = PX_PER_DAY[zoom];
  const [drag, setDrag] = useState<{ mode: Mode; startX: number; delta: number } | null>(null);
  const moved = useRef(false);
  const draggable = editable && !derived;
  const milestone = span.start === span.end;

  const shown = drag ? previewSpan(span, drag.mode, drag.delta) : span;
  const { x, width } = barBox(shown, from, zoom);
  const progress = progressOf(task);
  const late = !isDone(task) && shown.end < today;

  const begin = (mode: Mode) => (e: React.PointerEvent) => {
    if (!draggable || e.pointerType === "touch") return;
    e.preventDefault();
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    moved.current = false;
    setDrag({ mode, startX: e.clientX, delta: 0 });
  };
  const move = (e: React.PointerEvent) => {
    if (!drag) return;
    const delta = Math.round((e.clientX - drag.startX) / px);
    if (Math.abs(e.clientX - drag.startX) > 3) moved.current = true;
    if (delta !== drag.delta) {
      setDrag({ ...drag, delta });
      onPreview(task.id, previewSpan(span, drag.mode, delta));
    }
  };
  const end = () => {
    if (!drag) return;
    if (drag.delta !== 0) onCommit(task, drag.mode, drag.delta);
    onPreview(task.id, null);
    setDrag(null);
  };

  const common = {
    onPointerMove: move,
    onPointerUp: end,
    onPointerCancel: end,
    onClick: () => {
      if (moved.current) {
        moved.current = false;
        return;
      }
      onOpen();
    },
    title: `${task.title}${derived ? " (from subtasks)" : ""}`,
    "aria-label": task.title,
    role: "button",
    tabIndex: 0,
    onKeyDown: (e: React.KeyboardEvent) => e.key === "Enter" && onOpen(),
  } as const;

  if (milestone) {
    return (
      <div
        {...common}
        onPointerDown={begin("move")}
        className={cn(
          "absolute top-[9px] size-[14px] rotate-45 touch-none border",
          late
            ? "border-signal bg-signal"
            : isDone(task)
              ? "border-muted-foreground bg-muted-foreground"
              : "border-primary bg-primary",
          derived && "bg-transparent",
          draggable && "cursor-grab",
          drag && "cursor-grabbing opacity-80",
        )}
        style={{ left: x + px / 2 - 7 }}
      />
    );
  }

  return (
    <div
      {...common}
      onPointerDown={begin("move")}
      className={cn(
        "absolute top-[6px] flex h-[20px] touch-none overflow-hidden border",
        late
          ? "border-signal bg-signal/20"
          : isDone(task)
            ? "border-muted-foreground bg-muted"
            : "border-primary bg-accent",
        derived && "border-dashed bg-transparent",
        draggable && "cursor-grab",
        drag && "cursor-grabbing opacity-80",
      )}
      style={{ left: x, width }}
    >
      <div
        className={cn(
          "h-full",
          late ? "bg-signal" : isDone(task) ? "bg-muted-foreground" : "bg-primary",
        )}
        style={{ width: `${progress * 100}%`, opacity: derived ? 0.35 : 1 }}
      />
      {draggable ? (
        <>
          <span
            aria-hidden
            className="absolute inset-y-0 left-0 w-1.5 cursor-ew-resize"
            onPointerDown={begin("start")}
          />
          <span
            aria-hidden
            className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize"
            onPointerDown={begin("end")}
          />
        </>
      ) : null}
    </div>
  );
}
