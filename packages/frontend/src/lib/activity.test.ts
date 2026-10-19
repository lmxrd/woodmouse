import { describe, expect, it } from "vitest";
import type { ActivityView } from "woodmouse-sdk";

import { describeActivity, plainText } from "./activity";

const entry = (kind: string, data: Record<string, unknown>): ActivityView => ({
  id: "1",
  kind,
  data,
  created_at: "2026-10-16T08:30:00Z",
});

describe("plainText", () => {
  it("drops markdown markers and joins lines", () => {
    expect(plainText("Order **once** the layout:\n- electrician\n- plumber")).toBe(
      "Order once the layout: electrician plumber",
    );
    expect(plainText("# Title\n> quoted `code`")).toBe("Title quoted code");
  });
});

describe("describeActivity", () => {
  const day = (iso: string) => `day ${iso.slice(0, 10)}`;

  it("phrases the common kinds", () => {
    expect(describeActivity(entry("task_created", { title: "A" }), day).text).toBe('created "A"');
    expect(
      describeActivity(entry("task_moved", { title: "A", from: "To do", to: "Doing" }), day).text,
    ).toBe('moved "A" from To do to Doing');
    expect(
      describeActivity(entry("assigned", { title: "A", who: ["Sara", "Anders"] }), day).text,
    ).toBe('assigned "A" to Sara, Anders');
    expect(
      describeActivity(entry("link_added", { title: "A", kind: "blocks", other: "B" }), day).text,
    ).toBe('linked "A": it blocks "B"');
  });

  it("formats due dates with the given formatter and handles clearing", () => {
    expect(
      describeActivity(entry("due_changed", { title: "A", to: "2026-10-18T00:00:00Z" }), day).text,
    ).toBe('set the due date of "A" to day 2026-10-18');
    expect(describeActivity(entry("due_changed", { title: "A", to: null }), day).text).toBe(
      'cleared the due date of "A"',
    );
  });

  it("quotes a comment excerpt as plain text", () => {
    const { text, quote } = describeActivity(
      entry("comment_added", { title: "A", preview: "**17th**" }),
      day,
    );
    expect(text).toBe('commented on "A"');
    expect(quote).toBe("17th");
  });

  it("falls back for unknown kinds", () => {
    expect(describeActivity(entry("something_new", {}), day).text).toBe("something new");
  });
});
