import { defineConfig } from "@hey-api/openapi-ts";

// Regenerate with `mise run gen:client` (gen:spec -> openapi-ts, see
// mise.toml). Paths are relative to this package directory.
export default defineConfig({
  input: "../../../backend/target/specs/openapi.json",
  output: "src",
  plugins: ["@hey-api/client-fetch", "@tanstack/react-query"],
});
