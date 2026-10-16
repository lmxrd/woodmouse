import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, getRouteApi, useNavigate } from "@tanstack/react-router";
import type { TaskResponse } from "woodmouse-client";
import {
  listListsOptions,
  listTasksOptions,
  listWorkspaceLinksOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { addDays, daysBetween, parseIsoDay, toIsoDay } from "../../lib/calendar";
import { taskFilterQuery, type Zoom } from "../../lib/projectSearch";
import {
  GROUP_ROW_HEIGHT,
  MIN_CHART_DAYS,
  PX_PER_DAY,
  TASK_ROW_HEIGHT,
  buildRows,
  chartRange,
  dependencyArrows,
  ticks,
  todayStart,
  type Span,
} from "../../lib/timeline";
import { useFormatters } from "../../lib/useFormatters";
import { useMediaQuery } from "../../lib/useMediaQuery";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { cn } from "../../lib/utils";
import { ProjectFilters } from "../ProjectFilters";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { DependencyArrows, type Anchor } from "./DependencyArrows";
import { TimelineBar } from "./TimelineBar";
import { isDone } from "../../lib/task";
import { dayEpoch, dayIso } from "../../lib/time";

const route = getRouteApi("/w/$workspaceId/p/$projectId");
const LABEL_WIDTH_WIDE = 220;
const LABEL_WIDTH_NARROW = 130;
const HEADER_HEIGHT = 30;

/**
 * The project's tasks as a Gantt chart: a bar from start to due date, lists as groups, subtasks nested
 * (a parent without dates spans its children), dependency arrows from `blocks` links, and a today line.
 */
export function TimelineView({
  workspaceId,
  projectId,
}: {
  workspaceId: string;
  projectId: string;
}) {
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const openTask = useNavigate();
  const queryClient = useQueryClient();
  const { weekStart } = useFormatters();
  const { canWrite } = useWorkspaceRole(workspaceId);
  const [today] = useState(todayStart);
  const zoom: Zoom = search.zoom ?? "week";
  const anchorDay = parseIsoDay(search.date) ?? today;
  const px = PX_PER_DAY[zoom];
  const LABEL_WIDTH = useMediaQuery("(max-width: 639px)") ? LABEL_WIDTH_NARROW : LABEL_WIDTH_WIDE;

  const tasksQuery = listTasksOptions({
    path: { workspace_id: workspaceId },
    query: {
      project_id: projectId,
      include_subtasks: true,
      ...taskFilterQuery(search, "timeline"),
    },
  });
  const { data: tasks } = useQuery(tasksQuery);
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: edges } = useQuery(
    listWorkspaceLinksOptions({ path: { id: workspaceId }, query: { project_id: projectId } }),
  );

  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [previews, setPreviews] = useState<Map<string, Span>>(new Map());
  const rows = useMemo(
    () => buildRows(lists ?? [], tasks ?? [], collapsed),
    [lists, tasks, collapsed],
  );

  const spans = rows.flatMap((r) => (r.type === "task" && r.span ? [r.span] : []));
  const { from, to } = chartRange(spans, today, weekStart, MIN_CHART_DAYS[zoom]);
  const chartWidth = (daysBetween(from, to) + 1) * px;
  const axis = ticks(from, to, zoom, weekStart);

  // Geometry of every row, for the arrows.
  let y = 0;
  const anchors = new Map<string, Anchor>();
  const spanById = new Map<string, Span>();
  for (const row of rows) {
    const top = y;
    const height = row.type === "group" ? GROUP_ROW_HEIGHT : TASK_ROW_HEIGHT;
    if (row.type === "task" && row.span) {
      const shown = previews.get(row.task.id) ?? row.span;
      const left = daysBetween(from, shown.start) * px;
      const right = (daysBetween(from, shown.end) + 1) * px;
      anchors.set(row.task.id, { left, right, y: top + height / 2 });
      spanById.set(row.task.id, shown);
    }
    y += height;
  }
  const rowsHeight = y;
  const arrows = dependencyArrows(edges ?? [], spanById);
  const todayX = daysBetween(from, today) * px + px / 2;

  // The URL holds the zoom and the day at the center of the view, so a link opens at the same place.
  const scroller = useRef<HTMLDivElement>(null);
  const lastWritten = useRef<string | null>(null);
  const writeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const visibleChart = () => (scroller.current?.clientWidth ?? 0) - LABEL_WIDTH;
  const scrollToDay = (day: number) => {
    if (scroller.current) {
      scroller.current.scrollLeft = Math.max(0, daysBetween(from, day) * px - visibleChart() / 2);
    }
  };
  const centerDay = () =>
    addDays(from, Math.round(((scroller.current?.scrollLeft ?? 0) + visibleChart() / 2) / px));
  const ready = rows.length > 0;
  const anchorIso = toIsoDay(anchorDay);
  useEffect(() => {
    // Scroll to the URL's day, unless the URL just echoes where the user scrolled to.
    if (ready && lastWritten.current !== anchorIso) scrollToDay(anchorDay);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, zoom, from, anchorIso]);
  useEffect(() => {
    if (search.date === undefined || search.zoom !== zoom) {
      void navigate({
        search: (prev) => ({ ...prev, date: anchorIso, zoom }),
        replace: true,
      });
    }
  }, [search.date, search.zoom, zoom, anchorIso, navigate]);
  const onScroll = () => {
    clearTimeout(writeTimer.current);
    writeTimer.current = setTimeout(() => {
      const iso = toIsoDay(centerDay());
      if (iso === anchorIso) return;
      lastWritten.current = iso;
      void navigate({ search: (prev) => ({ ...prev, date: iso }), replace: true });
    }, 300);
  };
  const goToday = () => {
    lastWritten.current = null;
    void navigate({ search: (prev) => ({ ...prev, date: toIsoDay(today) }), replace: true });
    scrollToDay(today);
  };

  const update = useMutation({
    ...updateTaskMutation(),
    onMutate: async ({ path, body }) => {
      await queryClient.cancelQueries({ queryKey: tasksQuery.queryKey });
      queryClient.setQueryData<TaskResponse[]>(tasksQuery.queryKey, (old) =>
        old?.map((t) =>
          t.id === path.id
            ? {
                ...t,
                ...(body.start_date !== undefined ? { start_date: body.start_date } : {}),
                ...(body.due_date !== undefined ? { due_date: body.due_date } : {}),
              }
            : t,
        ),
      );
    },
    onSettled: () => queryClient.invalidateQueries(),
  });

  const commit = (task: TaskResponse, mode: "move" | "start" | "end", delta: number) => {
    const span = rows.find((r) => r.type === "task" && r.task.id === task.id);
    if (!span || span.type !== "task" || !span.span) return;
    const body: { start_date?: string; due_date?: string } = {};
    const start = dayEpoch(task.start_date);
    const due = dayEpoch(task.due_date);
    if (mode === "move") {
      if (start != null) body.start_date = dayIso(addDays(start, delta));
      if (due != null) body.due_date = dayIso(addDays(due, delta));
    } else if (mode === "start") {
      const next = addDays(start ?? span.span.start, delta);
      body.start_date = dayIso(Math.min(next, due ?? span.span.end));
    } else {
      const next = addDays(due ?? span.span.end, delta);
      body.due_date = dayIso(Math.max(next, start ?? span.span.start));
    }
    update.mutate({ path: { id: task.id }, body });
  };

  const gridLines =
    zoom === "month"
      ? undefined
      : `repeating-linear-gradient(90deg, transparent 0, transparent ${(zoom === "day" ? px : px * 7) - 1}px, var(--border) ${(zoom === "day" ? px : px * 7) - 1}px, var(--border) ${zoom === "day" ? px : px * 7}px)`;

  return (
    <>
      <ProjectFilters
        workspaceId={workspaceId}
        projectId={projectId}
        view="timeline"
        fields={["assignee", "label", "list", "status"]}
      />
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-2.5 sm:px-6">
        <p className="label-mono text-muted-foreground">
          Bars span start to due date · diamonds are single days · dashed bars come from subtasks
        </p>
        <div className="flex items-center gap-1.5">
          <Button type="button" variant="outline" size="sm" onClick={goToday}>
            Today
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
            <SelectTrigger size="sm" aria-label="Zoom" className="w-24">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="day">Day</SelectItem>
              <SelectItem value="week">Week</SelectItem>
              <SelectItem value="month">Month</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {rows.length === 0 ? (
        <p className="p-6 text-sm text-muted-foreground">No tasks yet. Add some on the board.</p>
      ) : null}

      <div
        ref={scroller}
        onScroll={onScroll}
        className="overflow-auto"
        style={{ maxHeight: "calc(100dvh - 190px)" }}
      >
        <div className="relative" style={{ width: LABEL_WIDTH + chartWidth }}>
          <div
            className="sticky top-0 z-30 flex border-b border-border bg-background"
            style={{ height: HEADER_HEIGHT }}
          >
            <div
              className="label-mono sticky left-0 z-30 flex items-center border-r border-border bg-background px-3 text-muted-foreground"
              style={{ width: LABEL_WIDTH }}
            >
              Task
            </div>
            <div className="relative" style={{ width: chartWidth }}>
              {axis.map((tick) => (
                <span
                  key={tick.day}
                  className="label-mono absolute top-0 flex h-full items-center border-l border-border pl-1 text-[10px] text-muted-foreground"
                  style={{ left: daysBetween(from, tick.day) * px }}
                >
                  {tick.label}
                </span>
              ))}
            </div>
          </div>

          {rows.map((row) =>
            row.type === "group" ? (
              <div
                key={`g-${row.list.id}`}
                className="flex border-b border-border bg-background"
                style={{ height: GROUP_ROW_HEIGHT }}
              >
                <div
                  className="label-mono sticky left-0 z-20 flex items-center truncate border-r border-border bg-background px-3 text-muted-foreground"
                  style={{ width: LABEL_WIDTH }}
                >
                  {row.list.name}
                </div>
                <div style={{ width: chartWidth }} />
              </div>
            ) : (
              <div
                key={row.task.id}
                className="flex border-b border-border"
                style={{ height: TASK_ROW_HEIGHT }}
              >
                <div
                  className="sticky left-0 z-20 flex items-center gap-1 border-r border-border bg-card px-2"
                  style={{ width: LABEL_WIDTH, paddingLeft: 8 + row.depth * 14 }}
                >
                  {row.hasChildren ? (
                    <button
                      type="button"
                      aria-label={
                        collapsed.has(row.task.id) ? "Expand subtasks" : "Collapse subtasks"
                      }
                      aria-expanded={!collapsed.has(row.task.id)}
                      className="w-4 shrink-0 text-xs text-muted-foreground"
                      onClick={() =>
                        setCollapsed((set) => {
                          const next = new Set(set);
                          if (!next.delete(row.task.id)) next.add(row.task.id);
                          return next;
                        })
                      }
                    >
                      {collapsed.has(row.task.id) ? "▸" : "▾"}
                    </button>
                  ) : (
                    <span className="w-4 shrink-0" />
                  )}
                  <Link
                    to="/w/$workspaceId/p/$projectId/t/$taskId"
                    params={{ workspaceId, projectId, taskId: row.task.id }}
                    search={(prev) => prev}
                    className={cn(
                      "min-w-0 flex-1 truncate text-sm hover:underline",
                      isDone(row.task) && "text-muted-foreground line-through",
                    )}
                  >
                    {row.task.title}
                  </Link>
                </div>
                <div
                  className="relative bg-card"
                  style={{ width: chartWidth, backgroundImage: gridLines }}
                >
                  {row.span ? (
                    <TimelineBar
                      task={row.task}
                      span={row.span}
                      derived={row.derived}
                      from={from}
                      zoom={zoom}
                      today={today}
                      editable={canWrite}
                      onPreview={(id, span) =>
                        setPreviews((old) => {
                          const next = new Map(old);
                          if (span) next.set(id, span);
                          else next.delete(id);
                          return next;
                        })
                      }
                      onCommit={commit}
                      onOpen={() =>
                        void openTask({
                          to: "/w/$workspaceId/p/$projectId/t/$taskId",
                          params: { workspaceId, projectId, taskId: row.task.id },
                          search: (prev) => prev,
                        })
                      }
                    />
                  ) : (
                    <span className="label-mono absolute top-2 left-3 text-[10px] text-muted-foreground">
                      No dates
                    </span>
                  )}
                </div>
              </div>
            ),
          )}

          <div
            className="pointer-events-none absolute z-10"
            style={{ left: LABEL_WIDTH, top: HEADER_HEIGHT, width: chartWidth, height: rowsHeight }}
          >
            <div className="absolute inset-y-0 border-l-2 border-signal" style={{ left: todayX }} />
            <DependencyArrows
              arrows={arrows}
              anchors={anchors}
              width={chartWidth}
              height={rowsHeight}
            />
          </div>
        </div>
      </div>
    </>
  );
}
