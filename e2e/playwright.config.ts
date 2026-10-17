import { defineConfig, devices } from '@playwright/test';

// Browsers to run: `MX_BROWSERS=chromium,webkit` (default: chromium).
const browsers = (process.env.MX_BROWSERS ?? 'chromium').split(',');

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    viewport: { width: 1400, height: 860 },
    trace: 'retain-on-failure',
    launchOptions: { args: process.env.CI ? [] : ['--no-sandbox'] },
  },
  projects: browsers.map((name) => ({
    name,
    use: {
      ...devices[
        name === 'webkit'
          ? 'Desktop Safari'
          : name === 'firefox'
            ? 'Desktop Firefox'
            : 'Desktop Chrome'
      ],
      viewport: { width: 1400, height: 860 },
      // Chromium-only flags must not reach the other engines.
      launchOptions: name === 'chromium' && !process.env.CI ? { args: ['--no-sandbox'] } : {},
    },
  })),
});
