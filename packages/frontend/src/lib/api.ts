import { client } from "woodmouse-sdk/client";

// TanStack Query base wrapper: configure the generated client once, before
// any request fires. Same-origin in production (the bundle is embedded in
// the API binary) and in development (vite proxies /api to the backend), so
// the session cookie travels automatically. VITE_API_URL points the client
// elsewhere when needed.
client.setConfig({
  baseUrl: import.meta.env.VITE_API_URL ?? "",
  credentials: "include",
});

let onSessionExpired: () => void = () => {};
let lastUnauthorizedAt = 0;

/** True just after a 401, so the failed request's own error toast can stay quiet. */
export function justLostSession(): boolean {
  return Date.now() - lastUnauthorizedAt < 2000;
}

/** Register what happens when the API says the session is gone (set once, at startup). */
export function setSessionExpiredHandler(handler: () => void) {
  onSessionExpired = handler;
}

/**
 * A 401 anywhere outside /api/auth/* means the session expired or was revoked. The auth calls are
 * excluded because there a 401 is an answer ("wrong password", "not logged in"), not a lost session.
 * Used by the API client below and by plain `fetch` calls (uploads).
 */
export function reportResponse(response: Response) {
  if (response.status === 401 && !new URL(response.url).pathname.startsWith("/api/auth/")) {
    lastUnauthorizedAt = Date.now();
    onSessionExpired();
  }
}

client.interceptors.response.use((response) => {
  reportResponse(response);
  return response;
});

/** The URL of a stored file's bytes (images can be used directly as `<img src>`). */
export function fileContentUrl(fileId: string): string {
  return `${import.meta.env.VITE_API_URL ?? ""}/api/files/${fileId}/content`;
}

/** The URL of one version's bytes. */
export function fileVersionContentUrl(fileId: string, versionId: string): string {
  return `${import.meta.env.VITE_API_URL ?? ""}/api/files/${fileId}/versions/${versionId}/content`;
}

/** Where a page downloads as a Markdown file. */
export function pageExportUrl(pageId: string): string {
  return `${import.meta.env.VITE_API_URL ?? ""}/api/pages/${pageId}/export`;
}

/** Where a workspace's whole wiki downloads as a zip of Markdown files. */
export function wikiExportUrl(workspaceId: string): string {
  return `${import.meta.env.VITE_API_URL ?? ""}/api/workspaces/${workspaceId}/wiki/export`;
}

/** A calendar link's feed address on this server (`https://...`). */
export function calendarFeedUrl(path: string): string {
  return `${import.meta.env.VITE_API_URL ?? window.location.origin}${path}`;
}
