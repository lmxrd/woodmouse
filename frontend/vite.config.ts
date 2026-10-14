import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    // File-based routing: generates src/routeTree.gen.ts from src/routes/.
    // Must run before the react plugin.
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
      quoteStyle: "single",
      semicolons: false,
    }),
    react(),
    tailwindcss(),
  ],
  server: {
    proxy: {
      // Same-origin cookies in development: the SPA talks to /api on the
      // vite origin and vite forwards it to the backend.
      "/api": "http://localhost:8000",
    },
  },
});
