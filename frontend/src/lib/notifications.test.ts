import { describe, expect, it } from "vitest";

import { describeNotification, notificationQuote } from "./notifications";

const actor = { name: "Sara", email: "sara@home.io" } as never;

describe("describeNotification", () => {
  it("phrases each kind", () => {
    expect(
      describeNotification({ kind: "task_assigned", actor, data: { title: "Order oak" } }),
    ).toBe('Sara assigned you to "Order oak"');
    expect(
      describeNotification({
        kind: "task_changed",
        actor,
        data: { title: "A", what: "completed" },
      }),
    ).toBe('Sara changed "A" (completed)');
    expect(describeNotification({ kind: "overdue", data: { title: "A" } })).toBe('"A" is overdue');
  });
  it("falls back to the email, then to Someone", () => {
    expect(
      describeNotification({
        kind: "page_changed",
        actor: { name: " ", email: "a@b.c" } as never,
        data: { title: "P" },
      }),
    ).toBe('a@b.c edited the page "P"');
    expect(describeNotification({ kind: "page_changed", data: {} })).toBe(
      'Someone edited the page "something"',
    );
  });
  it("quotes only comments", () => {
    expect(
      notificationQuote({ kind: "comment_added", data: { preview: "Back on the 17th" } }),
    ).toBe("Back on the 17th");
    expect(notificationQuote({ kind: "overdue", data: {} })).toBeUndefined();
  });
});
