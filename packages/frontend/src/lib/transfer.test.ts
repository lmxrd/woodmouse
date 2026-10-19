import { describe, expect, it } from "vitest";

import type { ImportReport } from "woodmouse-sdk";

import { canImport, exportCsvUrl, exportJsonUrl, importFormat } from "./transfer";

describe("transfer urls", () => {
  it("builds the export addresses", () => {
    expect(exportJsonUrl("w1", ["projects", "wiki"], [])).toBe(
      "/api/workspaces/w1/export?parts=projects%2Cwiki",
    );
    expect(exportJsonUrl("w1", ["projects"], ["a", "b"])).toContain("project_ids=a%2Cb");
    expect(exportCsvUrl("w1", [])).toBe("/api/workspaces/w1/export/tasks.csv");
    expect(exportCsvUrl("w1", ["a"])).toBe("/api/workspaces/w1/export/tasks.csv?project_ids=a");
  });
  it("tells the format from the file name", () => {
    expect(importFormat("Tasks.CSV")).toBe("csv");
    expect(importFormat("export.json")).toBe("json");
    expect(importFormat("notes.txt")).toBeUndefined();
  });
});

describe("canImport", () => {
  const report = (over: Partial<ImportReport>): ImportReport =>
    ({ problems: [], unmapped_users: [], warnings: [], invites: [], ...over }) as ImportReport;
  it("needs a preview with no problems and nobody left without a choice", () => {
    expect(canImport(null)).toBe(false);
    expect(canImport(report({}))).toBe(true);
    expect(canImport(report({ problems: ["row 3: no title"] }))).toBe(false);
    expect(canImport(report({ unmapped_users: [{ email: "a@b.c", references: 1 }] }))).toBe(false);
  });
});
