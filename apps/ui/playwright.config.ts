import { defineConfig } from "@playwright/test";
import path from "node:path";

// End-to-end (`e2e/flows`) and accessibility (`e2e/a11y`) tests run against the mockup build
// (`npm run build:mockup`), opened from the file system: no server, no network, deterministic data.
// `AIW_CHROMIUM` points at another Chromium binary (the CI installs the one Playwright wants).
const executablePath = process.env.AIW_CHROMIUM || undefined;
// `PW_NAME` keeps the artefacts of separate runs apart (the CI runs the E2E and the accessibility suites one after the other).
const name = process.env.PW_NAME || "local";

export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.spec.ts",
  outputDir: path.join("test-results", name),
  // Every test opens its own page: the demo state lives in memory, so tests cannot affect each other.
  fullyParallel: true,
  workers: process.env.CI ? 2 : undefined,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  forbidOnly: !!process.env.CI,
  reporter: [["list"], ["html", { open: "never", outputFolder: path.join("playwright-report", name) }]],
  use: {
    trace: "off",
    screenshot: "only-on-failure",
    video: "off",
    acceptDownloads: true,
    launchOptions: executablePath ? { executablePath } : {},
  },
  projects: [
    { name: "e2e", testDir: path.join("e2e", "flows"), use: { viewport: { width: 1440, height: 900 }, locale: "it-IT" } },
    { name: "a11y", testDir: path.join("e2e", "a11y"), use: { locale: "it-IT" } },
  ],
});
