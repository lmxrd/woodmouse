import { defineConfig, type UserConfig } from "@hey-api/openapi-ts";
import { resolve } from "node:path";

// Regenerate with `mise run gen:sdk` (gen:spec -> openapi-ts, see
// mise.toml). Paths are relative to this package directory.
export const config: UserConfig = {
  input: resolve(import.meta.dirname, "../backend/target/specs/openapi.json"),
  output: { path: resolve(import.meta.dirname, "src"), source: true },
  plugins: [
    "@hey-api/client-fetch",
    "@tanstack/react-query",
    { name: "zod", metadata: true, types: { infer: true } },
    {
      name: "@hey-api/sdk",
      // validator: true,
    },
  ],
};

export default defineConfig(config);
