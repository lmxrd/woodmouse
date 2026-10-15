import { useDroppable } from "@dnd-kit/core";
import type { List, TaskResponse } from "woodmouse-client";

import { assignLanes, daysBetween, taskSpan } from "../../lib/calendar";
import { cn } from "../../lib/utils";
import { CalendarEvent } from "./CalendarEvent";
import { QuickAdd } from "./QuickAdd";

const LANE_HEIGHT = 22;

function DayCell({
  day,
  today,
  muted,
  editable,
  lists,
}: {
  day: number;
  today: number;
  muted: boolean;
  editable: boolean;
  lists: List[];
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `day:${day}`, disabled: !editable });
  return (
    <div
      ref={setNodeRef}
      data-day={day}
      className={cn(
        "group/day flex items-start justify-between border-r border-border px-1.5 py-1",
        muted ? "bg-muted/60 text-muted-foreground" : "bg-card",
        day === today && "outline outline-2 -outline-offset-2 outline-primary",
        isOver && "bg-accent",
      )}
    >
      <span className={cn("font-mono text-[10px]", day === today && "font-medium text-primary")}>
        {new Date(day * 1000).getDate()}
      </span>
      {editable ? <QuickAdd day={day} lists={lists} /> : null}
    </div>
  );
}

/**
 * One week of the calendar: seven day cells (the drop targets) with the week's tasks laid over them.
 * A task spans from its start to its due date, clipped to the week and stacked so none overlap.
 */
export function WeekRow({
  days,
  tasks,
  today,
  month,
  tall,
  workspaceId,
  projectId,
  editable,
  lists,
}: {
  days: number[];
  tasks: TaskResponse[];
  today: number;
  /** The month being shown (0-11); days outside it are muted. Undefined for a plain week. */
  month?: number;
  tall?: boolean;
  workspaceId: string;
  projectId: string;
  editable: boolean;
  lists: List[];
}) {
  const [first, last] = [days[0], days[6]];
  const items = tasks.flatMap((task) => {
    const span = taskSpan(task);
    return span && span.end >= first && span.start <= last ? [{ task, span }] : [];
  });
  const clipped = items.map(({ span }) => ({
    start: Math.max(span.start, first),
    end: Math.min(span.end, last),
  }));
  const lanes = assignLanes(clipped);
  const laneCount = lanes.length ? Math.max(...lanes) + 1 : 0;
  const height = Math.max(tall ? 260 : 84, 26 + laneCount * LANE_HEIGHT + 8);

  return (
    <div className="relative grid grid-cols-7 border-b border-border" style={{ minHeight: height }}>
      {days.map((day) => (
        <DayCell
          key={day}
          day={day}
          today={today}
          muted={month !== undefined && new Date(day * 1000).getMonth() !== month}
          editable={editable}
          lists={lists}
        />
      ))}
      <div
        className="pointer-events-none absolute inset-x-0 top-6 grid grid-cols-7 gap-y-0.5"
        style={{ gridAutoRows: LANE_HEIGHT - 2 }}
      >
        {items.map(({ task, span }, i) => {
          const colStart = daysBetween(first, clipped[i].start);
          const length = daysBetween(clipped[i].start, clipped[i].end) + 1;
          return (
            <CalendarEvent
              key={task.id}
              task={task}
              today={today}
              end={span.end}
              workspaceId={workspaceId}
              projectId={projectId}
              editable={editable}
              continuesLeft={span.start < first}
              continuesRight={span.end > last}
              className="pointer-events-auto mx-0.5"
              style={{ gridColumn: `${colStart + 1} / span ${length}`, gridRow: lanes[i] + 1 }}
            />
          );
        })}
      </div>
    </div>
  );
}
