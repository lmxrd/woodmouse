import { client } from "woodmouse-client/client";

// TanStack Query base wrapper: configure the generated client once, before
// any request fires. Same-origin in production (the bundle is embedded in
// the API binary) and in development (vite proxies /api to the backend), so
// the session cookie travels automatically. VITE_API_URL points the client
// elsewhere when needed.
client.setConfig({
  baseUrl: import.meta.env.VITE_API_URL ?? "",
  credentials: "include",
});
