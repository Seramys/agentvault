import { defineConfig } from '@playwright/test';

const port = Number(process.env.AGENTVAULT_E2E_PORT || 4317);

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: 'list',
  use: {
    baseURL: process.env.AGENTVAULT_E2E_BASE_URL || `http://127.0.0.1:${port}`,
    channel: 'chrome',
    headless: true,
    trace: 'retain-on-failure',
    launchOptions: {
      args: [
        '--no-sandbox',
        '--disable-crash-reporter',
        '--disable-breakpad',
        '--single-process',
        '--no-zygote',
        '--disable-gpu',
      ],
    },
  },
});