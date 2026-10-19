import { describe, expect, it } from "vitest";
import type { LinkEdge, List, TaskResponse } from "woodmouse-client";

import { addDays, daysBetween } from "./calendar";
import { dayIso } from "./time";
import {
  barBox,
  buildRows,
  chartRange,
  dependencyArrows,
  progressOf,
  ticks,
  violatesDependency,
} from "./timeline";

const day = (y: number, m: number, d: number) => Math.floor(new Date(y, m - 1, d).getTime() / 1000);
const iso = (epochDay: number) => dayIso(epochDay);
const list = (id: string, name = id): List => ({ id, name, project_id: "p", position: 20 });
const task = (id: string, extra: Partial<TaskResponse> = {}): TaskResponse =>
  ({
    id,
    list_id: "l1",
    parent_id: null,
    title: id,
    position: 1,
    completed_at: null,
    subtask_total: 0,
    subtask_completed: 0,
    ...extra,
  }) as TaskResponse;

describe("buildRows", () => {
  const lists = [list("l1", "To do"), list("empty")];

  it("groups by list, skips empty lists, and orders by position", () => {
    const rows = buildRows(
      lists,
      [task("b", { position: 2 }), task("a", { position: 1 }), task("i", { list_id: null })],
      new Set(),
    );
    expect(rows.map((r) => (r.type === "group" ? `# ${r.list.name}` : r.task.id))).toEqual([
      "# Inbox",
      "i",
      "# To do",
      "a",
      "b",
    ]);
  });

  it("nests subtasks and hides them when the parent is collapsed", () => {
    const tasks = [
      task("p"),
      task("c1", { parent_id: "p", position: 1 }),
      task("c2", { parent_id: "p", position: 2 }),
      task("g", { parent_id: "c1" }),
    ];
    const open = buildRows(lists, tasks, new Set());
    expect(
      open
        .filter((r) => r.type === "task")
        .map((r) => (r.type === "task" ? `${r.depth}:${r.task.id}` : "")),
    ).toEqual(["0:p", "1:c1", "2:g", "1:c2"]);
    const collapsed = buildRows(lists, tasks, new Set(["p"]));
    expect(collapsed.filter((r) => r.type === "task")).toHaveLength(1);
  });

  it("derives a parent's span from its subtasks when it has no dates", () => {
    const [a, b, c] = [day(2026, 10, 12), day(2026, 10, 15), day(2026, 10, 20)];
    const rows = buildRows(
      lists,
      [
        task("p"),
        task("c1", { parent_id: "p", start_date: iso(a), due_date: iso(b) }),
        task("c2", { parent_id: "p", due_date: iso(c) }),
        task("own", { start_date: iso(a), due_date: iso(b) }),
        task("kid", { parent_id: "own", start_date: iso(c), due_date: iso(c) }),
      ],
      new Set(),
    );
    const byId = new Map(rows.flatMap((r) => (r.type === "task" ? [[r.task.id, r] as const] : [])));
    expect(byId.get("p")).toMatchObject({ derived: true, span: { start: a, end: c } });
    expect(byId.get("own")).toMatchObject({ derived: false, span: { start: a, end: b } });
    expect(byId.get("c2")).toMatchObject({ derived: false, span: { start: c, end: c } });
  });

  it("shows a subtask whose parent was filtered out at the top level", () => {
    const rows = buildRows(lists, [task("kid", { parent_id: "missing-parent" })], new Set());
    expect(rows.map((r) => (r.type === "group" ? "group" : `${r.depth}:${r.task.id}`))).toEqual([
      "group",
      "0:kid",
    ]);
  });

  it("leaves a task with no dates anywhere without a span", () => {
    const rows = buildRows(lists, [task("x")], new Set());
    expect(rows[1]).toMatchObject({ type: "task", span: null });
  });
});

describe("chart geometry", () => {
  const today = day(2026, 10, 15);

  it("pads around the tasks and is at least two months wide", () => {
    const { from, to } = chartRange([], today, "monday");
    expect(daysBetween(from, to)).toBeGreaterThanOrEqual(60);
    expect(from).toBeLessThan(today);
    expect(new Date(from * 1000).getDay()).toBe(1); // starts on a Monday
    const wide = chartRange([{ start: day(2026, 1, 5), end: day(2026, 12, 20) }], today, "monday");
    expect(wide.from).toBeLessThan(day(2026, 1, 5));
    expect(wide.to).toBeGreaterThan(day(2026, 12, 20));
  });

  it("positions bars by day", () => {
    const from = day(2026, 10, 1);
    expect(barBox({ start: day(2026, 10, 3), end: day(2026, 10, 5) }, from, "day")).toEqual({
      x: 72,
      width: 108,
    });
    expect(barBox({ start: from, end: from }, from, "week")).toEqual({ x: 0, width: 14 });
  });

  it("labels the axis per zoom", () => {
    const from = day(2026, 10, 26);
    const to = day(2026, 11, 8);
    const days = ticks(from, to, "day", "monday");
    expect(days).toHaveLength(14);
    expect(days.find((t) => t.day === day(2026, 11, 1))?.label).toBe("1 Nov");
    expect(ticks(from, to, "week", "monday").map((t) => t.label)).toEqual(["26 Oct", "2 Nov"]);
    const months = ticks(day(2026, 10, 20), day(2027, 1, 10), "month", "monday").map(
      (t) => t.label,
    );
    expect(months).toEqual(["Oct 26", "Nov 26", "Dec 26", "Jan 27"]);
  });
});

describe("dependencies", () => {
  const edge = (
    task_id: string,
    related_task_id: string,
    kind: LinkEdge["kind"] = "blocks",
  ): LinkEdge => ({
    id: `${task_id}-${related_task_id}`,
    task_id,
    related_task_id,
    kind,
  });
  const a = day(2026, 10, 12);

  it("flags a blocked task that starts before its blocker ends", () => {
    const blocker = { start: a, end: addDays(a, 3) };
    expect(violatesDependency(blocker, { start: addDays(a, 4), end: addDays(a, 6) })).toBe(false);
    expect(violatesDependency(blocker, { start: addDays(a, 3), end: addDays(a, 6) })).toBe(true);
    expect(violatesDependency(blocker, { start: addDays(a, 1), end: addDays(a, 2) })).toBe(true);
  });

  it("draws only blocks links between tasks that both have spans", () => {
    const spans = new Map([
      ["a", { start: a, end: addDays(a, 2) }],
      ["b", { start: addDays(a, 5), end: addDays(a, 6) }],
      ["c", { start: addDays(a, 1), end: addDays(a, 1) }],
    ]);
    const arrows = dependencyArrows(
      [edge("a", "b"), edge("a", "c"), edge("a", "missing"), edge("a", "b", "relates_to")],
      spans,
    );
    expect(arrows.map((x) => [x.blocker, x.blocked, x.violated])).toEqual([
      ["a", "b", false],
      ["a", "c", true],
    ]);
  });
});

describe("progressOf", () => {
  it("uses completed subtasks, else the completed state", () => {
    expect(progressOf(task("a", { subtask_total: 4, subtask_completed: 1 }))).toBe(0.25);
    expect(progressOf(task("a", { completed_at: "2026-10-16T08:30:00Z" }))).toBe(1);
    expect(progressOf(task("a"))).toBe(0);
  });
});
