import { describe, expect, it } from "vitest";

import { DEFAULT_STATUS, taskFilterQuery, validateProjectSearch } from "./projectSearch";

describe("validateProjectSearch", () => {
  it("keeps every valid view parameter", () => {
    const search = {
      view: "timeline",
      status: "all",
      assignee: "u1",
      label: "l1",
      list: "li1",
      sort: "due_date",
      order: "desc",
      date: "2026-10-15",
      zoom: "month",
    };
    expect(validateProjectSearch(search)).toEqual(search);
  });

  it("drops anything unknown or invalid instead of failing", () => {
    expect(
      validateProjectSearch({
        view: "gantt",
        status: "maybe",
        date: "2026-02-30",
        zoom: "year",
        assignee: "",
        sort: 7,
      }),
    ).toEqual({
      view: undefined,
      status: undefined,
      assignee: undefined,
      label: undefined,
      list: undefined,
      sort: undefined,
      order: undefined,
      date: undefined,
      zoom: undefined,
    });
  });

  it("is idempotent, so a URL round-trips to the same state", () => {
    const once = validateProjectSearch({
      view: "calendar",
      date: "2026-10-15",
      zoom: "week",
      label: "x",
    });
    expect(validateProjectSearch({ ...once })).toEqual(once);
  });
});

describe("taskFilterQuery", () => {
  it("maps URL filters to backend filters", () => {
    expect(
      taskFilterQuery({ assignee: "u", label: "l", list: "li", status: "done" }, "list"),
    ).toEqual({ assignee_id: "u", label_id: "l", list_id: "li", is_completed: true });
  });

  it("applies each view's default status when the URL has none", () => {
    expect(taskFilterQuery({}, "list").is_completed).toBe(false);
    expect(taskFilterQuery({}, "calendar").is_completed).toBeUndefined();
    expect(taskFilterQuery({}, "timeline").is_completed).toBeUndefined();
    expect(taskFilterQuery({}, "board").is_completed).toBeUndefined();
    expect(DEFAULT_STATUS.list).toBe("open");
  });

  it("lets an explicit status override the default", () => {
    expect(taskFilterQuery({ status: "all" }, "list").is_completed).toBeUndefined();
    expect(taskFilterQuery({ status: "open" }, "calendar").is_completed).toBe(false);
  });
});
