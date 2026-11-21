import { defineConfig, devices } from '@playwright/test'

// Runs against the real server binary with the simulated camera and mount (never touches hardware).
//   npm run build && cargo build --release -p rustflare-server && npx playwright test
// Against a real Pi instead (read-only checks only):  E2E_URL=http://astropi.local:8080 npx playwright test --grep @pi
const external = process.env.E2E_URL
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  workers: 1,
  reporter: [['list']],
  use: { baseURL: external ?? 'http://127.0.0.1:18101', trace: 'retain-on-failure' },
  projects: [
    { name: 'phone', use: { ...devices['Pixel 7'] } },
    { name: 'desktop', use: { viewport: { width: 1280, height: 800 } } },
  ],
  webServer: external ? undefined : {
    command: 'rm -rf .e2e-data && ../target/release/rustflare-server --bind 127.0.0.1:18101 --web-dir dist --data-dir .e2e-data/data --config-file .e2e-data/cfg.json',
    url: 'http://127.0.0.1:18101/api/status',
    reuseExistingServer: false,
    timeout: 20_000,
  },
})
