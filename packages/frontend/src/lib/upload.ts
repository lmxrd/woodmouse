import type { FileView } from "woodmouse-sdk";

import { reportResponse } from "./api";

export type UploadResult =
  | { status: "created"; file: FileView }
  | { status: "duplicate"; existing: FileView }
  | { status: "too_large" }
  | { status: "error" };

/** Upload a file to a workspace's library, optionally attaching it to a task. */
export async function uploadFile(
  workspaceId: string,
  file: File,
  taskId?: string,
): Promise<UploadResult> {
  const body = new FormData();
  if (taskId) body.append("task_id", taskId);
  body.append("file", file, file.name);
  try {
    const response = await fetch(
      `${import.meta.env.VITE_API_URL ?? ""}/api/workspaces/${workspaceId}/files`,
      { method: "POST", body, credentials: "include" },
    );
    reportResponse(response);
    if (response.status === 201) return { status: "created", file: await response.json() };
    if (response.status === 409) {
      const duplicate = await response.json();
      return { status: "duplicate", existing: duplicate.existing_file };
    }
    if (response.status === 413) return { status: "too_large" };
    return { status: "error" };
  } catch {
    return { status: "error" };
  }
}

/** Upload a new version of a library file (same file id and name). */
export async function uploadFileVersion(fileId: string, file: File): Promise<UploadResult> {
  const body = new FormData();
  body.append("file", file, file.name);
  try {
    const response = await fetch(
      `${import.meta.env.VITE_API_URL ?? ""}/api/files/${fileId}/versions`,
      { method: "POST", body, credentials: "include" },
    );
    reportResponse(response);
    if (response.status === 201) return { status: "created", file: await response.json() };
    if (response.status === 409) return { status: "duplicate", existing: await response.json() };
    if (response.status === 413) return { status: "too_large" };
    return { status: "error" };
  } catch {
    return { status: "error" };
  }
}

/** "214 KB", "1.2 MB". */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
