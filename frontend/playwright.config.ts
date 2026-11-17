/**
 * The end-to-end lane of the console.
 *
 * The console is tested against the **real service**: the same binary, the
 * same Cedar bundle and the same catalog that a deployment runs, with the
 * data seeded through the API. A console that is tested against a stand-in
 * proves that the console can talk to the stand-in.
 *
 * The service and the development server are started here, and the
 * development server points at the service, so the browser only ever
 * talks to one origin.
 *
 * The lane has its own ports and never reuses what it finds: a
 * development server that somebody else started talks to another service,
 * and a test that runs against it measures that other service. A lane that
 * measures the wrong thing is worse than a lane that does not run.
 */

import { defineConfig, devices } from '@playwright/test';

/** The port the service of the fixture listens on. */
const servicePort = Number(process.env.HODEI_FIXTURE_PORT ?? 18887);

/** Where the service of the fixture listens. */
const service = process.env.HODEI_FIXTURE_URL ?? `http://127.0.0.1:${servicePort}`;

/** Where the fixture announces that the catalog is seeded. */
const readiness =
  process.env.HODEI_FIXTURE_READY ?? `http://127.0.0.1:${servicePort + 1}`;

/** The port the development server of the lane listens on. */
const consolePort = Number(process.env.HODEI_CONSOLE_PORT ?? 5199);

/** Where the development server listens. */
const console_ = process.env.HODEI_CONSOLE_URL ?? `http://127.0.0.1:${consolePort}`;

export default defineConfig({
  testDir: './tests/e2e',
  // The service is started once and the data is seeded once: the tests
  // read the same catalog, and a test that seeds its own data would
  // prove less about the console than about the seeding.
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: [['list']],
  use: {
    baseURL: console_,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: [
    {
      command: 'node tests/e2e/fixture/backend.mjs',
      // The fixture announces that it is ready when the catalog is
      // seeded, and not only when the process is up: a test that starts
      // against a half-seeded catalog measures the seeding.
      url: readiness,
      reuseExistingServer: false,
      timeout: 120_000,
      stdout: 'pipe',
      stderr: 'pipe',
      env: {
        HODEI_FIXTURE_PORT: String(servicePort),
      },
    },
    {
      command: `npm run dev -- --port ${consolePort} --strictPort`,
      url: console_,
      reuseExistingServer: false,
      timeout: 120_000,
      env: {
        VITE_API_BASE_URL: service,
        VITE_OPEN: 'false',
      },
    },
  ],
});
