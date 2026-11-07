import { defineConfig, devices } from "@playwright/test";

// The smoke tests run against a server that is already up: `mise run test:e2e` builds the image, starts it
// with a fresh database file in a volume and sets E2E_BASE_URL. The flows build on each other (the first
// creates the admin), so they run in order, in one worker.
export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:8180",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
