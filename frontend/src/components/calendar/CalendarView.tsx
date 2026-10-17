import { useEffect, useRef, useState } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  TouchSensor,
  pointerWithin,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, getRouteApi } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";
import {
  listListsOptions,
  listTasksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import {
  addDays,
  daysBetween,
  monthGrid,
  parseIsoDay,
  shiftDates,
  startOfDay,
  taskSpan,
  toIsoDay,
  weekDays,
  weekdayLabels,
} from "../../lib/calendar";
import { taskFilterQuery, type Zoom } from "../../lib/projectSearch";
import { dayIso } from "../../lib/time";
import { useFormatters } from "../../lib/useFormatters";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { cn } from "../../lib/utils";
import { ProjectFilters } from "../views/ProjectFilters";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { eventTone } from "./CalendarEvent";
import { UnscheduledPanel } from "./UnscheduledPanel";
import { WeekRow } from "./WeekRow";

const route = getRouteApi("/w/$workspaceId/p/$projectId");

/** The day under the pointer when a drag began (the day cell it was picked up from). */
function dayAtPointer(event: Event | null): number | undefined {
  const point =
    event && "clientX" in event
      ? { x: (event as MouseEvent).clientX, y: (event as MouseEvent).clientY }
      : event && "touches" in event
        ? {
            x: (event as TouchEvent).touches[0]?.clientX ?? 0,
            y: (event as TouchEvent).touches[0]?.clientY ?? 0,
          }
        : null;
  if (!point) return undefined;
  for (const el of document.elementsFromPoint(point.x, point.y)) {
    const day = (el as HTMLElement).closest?.("[data-day]")?.getAttribute("data-day");
    if (day) return Number(day);
  }
  return undefined;
}

/** A month or week of the project's tasks on their dates, with drag-to-reschedule and quick add. */
export function CalendarView({
  workspaceId,
  projectId,
}: {
  workspaceId: string;
  projectId: string;
}) {
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const queryClient = useQueryClient();
  const { weekStart, localDay: formatDate } = useFormatters();
  const { canWrite } = useWorkspaceRole(workspaceId);
  const [today] = useState(() => startOfDay(Math.floor(Date.now() / 1000)));

  const zoom: Zoom = search.zoom === "week" ? "week" : "month";
  const anchor = parseIsoDay(search.date) ?? today;
  const weeks = zoom === "month" ? monthGrid(anchor, weekStart) : [weekDays(anchor, weekStart)];
  const [from, to] = [weeks[0][0], weeks[weeks.length - 1][6]];

  const filters = taskFilterQuery(search, "calendar");
  const rangeQuery = listTasksOptions({
    path: { workspace_id: workspaceId },
    query: {
      project_id: projectId,
      include_subtasks: true,
      from: dayIso(from),
      to: dayIso(to),
      ...filters,
    },
  });
  // The panel offers tasks to schedule, so it shows open ones unless a status filter says otherwise.
  const unscheduledQuery = listTasksOptions({
    path: { workspace_id: workspaceId },
    query: {
      project_id: projectId,
      unscheduled: true,
      ...filters,
      is_completed: filters.is_completed ?? false,
    },
  });
  const { data: tasks } = useQuery(rangeQuery);
  const { data: unscheduled } = useQuery(unscheduledQuery);
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));

  // Reschedules show at once; the server's answer replaces them when it arrives.
  const update = useMutation({
    ...updateTaskMutation(),
    onMutate: async ({ path, body }) => {
      await queryClient.cancelQueries();
      const dates = {
        ...(body.start_date !== undefined ? { start_date: body.start_date } : {}),
        ...(body.due_date !== undefined ? { due_date: body.due_date } : {}),
      };
      queryClient.setQueryData<TaskResponse[]>(rangeQuery.queryKey, (old) => {
        if (!old) return old;
        if (old.some((t) => t.id === path.id)) {
          return old.map((t) => (t.id === path.id ? { ...t, ...dates } : t));
        }
        const moved = unscheduled?.find((t) => t.id === path.id);
        return moved ? [...old, { ...moved, ...dates }] : old;
      });
      queryClient.setQueryData<TaskResponse[]>(unscheduledQuery.queryKey, (old) =>
        old?.filter((t) => t.id !== path.id),
      );
    },
    onSettled: () => queryClient.invalidateQueries(),
  });

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 250, tolerance: 8 } }),
  );
  const [dragging, setDragging] = useState<TaskResponse | null>(null);
  const grabDay = useRef<number | undefined>(undefined);

  const find = (id: string) =>
    tasks?.find((t) => t.id === id) ?? unscheduled?.find((t) => t.id === id);
  const onDragStart = (event: DragStartEvent) => {
    setDragging(find(String(event.active.id)) ?? null);
    grabDay.current = dayAtPointer(event.activatorEvent);
  };
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setDragging(null);
    const task = find(String(active.id));
    if (!task || !over || !String(over.id).startsWith("day:")) return;
    const target = Number(String(over.id).slice(4));
    const span = taskSpan(task);
    if (!span) {
      update.mutate({ path: { id: task.id }, body: { due_date: dayIso(target) } });
      return;
    }
    const delta = daysBetween(grabDay.current ?? span.start, target);
    if (delta !== 0) update.mutate({ path: { id: task.id }, body: shiftDates(task, delta) });
  };

  // Links must show the same thing whenever they are opened, so the day and zoom are always in the URL.
  const go = (date: number) =>
    void navigate({ search: (prev) => ({ ...prev, date: toIsoDay(date), zoom }), replace: true });
  const canonical = search.date !== undefined && search.zoom === zoom;
  useEffect(() => {
    if (!canonical) {
      void navigate({
        search: (prev) => ({ ...prev, date: toIsoDay(anchor), zoom }),
        replace: true,
      });
    }
  }, [canonical, anchor, zoom, navigate]);
  const step = (direction: 1 | -1) => {
    const d = new Date(anchor * 1000);
    go(
      zoom === "month"
        ? Math.floor(new Date(d.getFullYear(), d.getMonth() + direction, 1).getTime() / 1000)
        : addDays(anchor, 7 * direction),
    );
  };

  const title =
    zoom === "month"
      ? new Date(anchor * 1000).toLocaleString("en", { month: "long", year: "numeric" })
      : `${formatDate(weeks[0][0])} – ${formatDate(weeks[0][6])}`;
  const month = zoom === "month" ? new Date(anchor * 1000).getMonth() : undefined;

  // The agenda (phones) lists the days of the range that have tasks.
  const byDay = new Map<number, TaskResponse[]>();
  for (const task of tasks ?? []) {
    const span = taskSpan(task);
    if (!span) continue;
    const day = Math.max(span.start, from);
    byDay.set(day, [...(byDay.get(day) ?? []), task]);
  }
  const agenda = [...byDay.entries()].sort(([a], [b]) => a - b);

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={pointerWithin}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragCancel={() => setDragging(null)}
    >
      <ProjectFilters
        workspaceId={workspaceId}
        projectId={projectId}
        view="calendar"
        fields={["assignee", "label", "list", "sprint", "status"]}
      />
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-2.5 sm:px-6">
        <h2 className="font-display text-lg font-bold">{title}</h2>
        <div className="flex items-center gap-1.5">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label="Previous"
            onClick={() => step(-1)}
          >
            ‹
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={() => go(today)}>
            Today
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label="Next"
            onClick={() => step(1)}
          >
            ›
          </Button>
          <Select
            value={zoom}
            onValueChange={(z) =>
              void navigate({
                search: (prev) => ({ ...prev, zoom: z as Zoom }),
                replace: true,
              })
            }
          >
            <SelectTrigger size="sm" aria-label="Zoom" className="ml-1 w-24">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="month">Month</SelectItem>
              <SelectItem value="week">Week</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <div className="flex">
        <div className="hidden min-w-0 flex-1 md:block">
          <div className="grid grid-cols-7 border-b border-border bg-background">
            {weekdayLabels(weekStart).map((label) => (
              <div key={label} className="label-mono px-2 py-1.5 text-muted-foreground">
                {label}
              </div>
            ))}
          </div>
          {weeks.map((days) => (
            <WeekRow
              key={days[0]}
              days={days}
              tasks={tasks ?? []}
              today={today}
              month={month}
              tall={zoom === "week"}
              workspaceId={workspaceId}
              projectId={projectId}
              editable={canWrite}
              lists={lists ?? []}
            />
          ))}
          <p className="label-mono px-4 py-2 text-[10px] text-muted-foreground sm:px-6">
            {canWrite
              ? "Drag a task to another day to change its due date · tasks with a start date span their days"
              : "Tasks with a start date span their days"}
          </p>
        </div>
        <UnscheduledPanel
          tasks={unscheduled ?? []}
          workspaceId={workspaceId}
          projectId={projectId}
          editable={canWrite}
        />
      </div>

      <div className="md:hidden" aria-label="Agenda">
        {agenda.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">Nothing scheduled in this period.</p>
        ) : null}
        {agenda.map(([day, dayTasks]) => (
          <section key={day}>
            <h3
              className={cn(
                "label-mono border-b border-border bg-background px-4 py-1.5",
                day === today ? "text-primary" : "text-muted-foreground",
              )}
            >
              {day === today ? "Today · " : ""}
              {formatDate(day)}
            </h3>
            {dayTasks.map((task) => (
              <Link
                key={task.id}
                to="/w/$workspaceId/p/$projectId/t/$taskId"
                params={{ workspaceId, projectId, taskId: task.id }}
                search={(prev) => prev}
                className={cn(
                  "block border-b border-l-[3px] border-b-border bg-card px-4 py-2.5 text-sm",
                  eventTone(task, today, taskSpan(task)?.end ?? day),
                )}
              >
                {task.title}
              </Link>
            ))}
          </section>
        ))}
      </div>

      <DragOverlay>
        {dragging ? (
          <div className="border-l-[3px] border-l-primary bg-accent px-2 text-xs leading-6 shadow-md">
            {dragging.title}
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
