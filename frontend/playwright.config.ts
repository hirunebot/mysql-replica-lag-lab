import { defineConfig } from '@playwright/test';

const baseURL = 'http://127.0.0.1:4177';

export default defineConfig({
  testDir: 'tests/browser',
  use: { baseURL },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 900 } } },
    { name: 'mobile', use: { viewport: { width: 390, height: 844 }, hasTouch: true } },
  ],
  webServer: {
    command: 'npm run dev -- --port 4177 --strictPort',
    url: baseURL,
    reuseExistingServer: false,
  },
});
