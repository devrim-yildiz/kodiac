import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: "http://127.0.0.1:18197",
    viewport: { width: 1440, height: 1000 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  reporter: [["list"], ["json", { outputFile: "test-results/results.json" }]],
  webServer: {
    command:
      process.env.SPA_TEST_COMMAND || "python3 ../scripts/spa-test-server.py",
    url: "http://127.0.0.1:18198",
    timeout: 120_000,
    reuseExistingServer: false,
  },
});
