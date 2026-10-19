import type { ImportReport, UserChoice } from "woodmouse-sdk";

import { reportResponse } from "./api";

const base = () => import.meta.env.VITE_API_URL ?? "";

/** The download address of a workspace's JSON export (chosen parts, optionally some projects). */
export function exportJsonUrl(workspaceId: string, parts: string[], projectIds: string[]): string {
  const query = new URLSearchParams({ parts: parts.join(",") });
  if (projectIds.length > 0) query.set("project_ids", projectIds.join(","));
  return `${base()}/api/workspaces/${workspaceId}/export?${query}`;
}

/** The download address of the tasks CSV (every project, or only some). */
export function exportCsvUrl(workspaceId: string, projectIds: string[]): string {
  const query = projectIds.length > 0 ? `?project_ids=${projectIds.join(",")}` : "";
  return `${base()}/api/workspaces/${workspaceId}/export/tasks.csv${query}`;
}

/** `csv` or `json` from a file's name, or undefined for anything else. */
export function importFormat(fileName: string): "csv" | "json" | undefined {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".csv")) return "csv";
  if (lower.endsWith(".json")) return "json";
  return undefined;
}

/** What to do about each person in the file who is not a member (by lowercase email). */
export type UserMap = Record<string, UserChoice>;

export type ImportResult = { ok: true; report: ImportReport } | { ok: false; message: string };

/**
 * Send a file's text to the server's importer. With `dryRun` it only reports; a real import writes
 * everything or nothing, and when something blocks it the server answers 422 with the report.
 */
export async function importText(
  workspaceId: string,
  format: "csv" | "json",
  text: string,
  options: { dryRun: boolean; project?: string; userMap?: UserMap },
): Promise<ImportResult> {
  try {
    const response = await fetch(`${base()}/api/workspaces/${workspaceId}/import`, {
      method: "POST",
      body: JSON.stringify({
        format,
        text,
        dry_run: options.dryRun,
        project: options.project ?? null,
        user_map: options.userMap ?? {},
      }),
      headers: { "content-type": "application/json" },
      credentials: "include",
    });
    reportResponse(response);
    if (response.ok || response.status === 422) {
      return { ok: true, report: await response.json() };
    }
    const body = await response.json().catch(() => null);
    return { ok: false, message: body?.error ?? "The server could not read that file." };
  } catch {
    return { ok: false, message: "Couldn't reach the server. Try again." };
  }
}

/** Whether a report allows importing: no problems, and everyone not in the workspace has a choice. */
export function canImport(report: ImportReport | null): boolean {
  return !!report && report.problems.length === 0 && report.unmapped_users.length === 0;
}
