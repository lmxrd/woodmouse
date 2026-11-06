import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { heyApiPlugin } from "@hey-api/vite-plugin";
import { config } from "../sdk/openapi-ts.config.ts";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    // Regenerates the SDK on dev and build. Not under Vitest: `mise run test` runs it next to the
    // backend tests, whose frontend build reads the SDK this would clean and rewrite.
    ...(process.env.VITEST ? [] : [heyApiPlugin({ config: config })]),
    // File-based routing: generates src/routeTree.gen.ts from src/routes/.
    // Must run before the react plugin.
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
      quoteStyle: "double",
      semicolons: true,
    }),
    react(),
    tailwindcss(),
  ],
  server: {
    proxy: {
      // Same-origin cookies in development: the SPA talks to /api on the
      // vite origin and vite forwards it to the backend.
      "/api": { target: "http://localhost:8000", ws: true },
    },
  },
});
