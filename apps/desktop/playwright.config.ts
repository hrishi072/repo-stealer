import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';

export default defineConfig({
  testDir: './tests', timeout: 30_000, fullyParallel: true,
  use: { baseURL: 'http://127.0.0.1:1420', channel: 'chrome', headless: true, viewport: { width: 1360, height: 1000 }, launchOptions: { chromiumSandbox: true, executablePath: process.env.RS_CHROME_PATH || (existsSync('/usr/bin/google-chrome') ? '/usr/bin/google-chrome' : undefined) } },
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI },
  reporter: [['list']],
});
