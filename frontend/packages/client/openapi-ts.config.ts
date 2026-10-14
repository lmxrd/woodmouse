import { defineConfig } from "@hey-api/openapi-ts";

// Regenerate with `scripts/gen-client.sh` (runs `cargo specs` first, then
// openapi-ts). Paths are relative to this package directory.
export default defineConfig({
  input: "../../../backend/target/specs/openapi.json",
  output: "src",
  plugins: ["@hey-api/client-fetch", "@tanstack/react-query"],
});
