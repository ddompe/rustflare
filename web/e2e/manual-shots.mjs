// Captures the screenshots used by docs/manual.md from the simulator.
//   npm run build && cargo build --release -p rustflare-server
//   rm -rf .shots && ../target/release/rustflare-server --bind 127.0.0.1:18120 --web-dir dist --data-dir .shots/data --config-file .shots/c.json &
//   node e2e/manual-shots.mjs ../docs/img/manual
import { chromium, devices } from '@playwright/test'
const base = 'http://127.0.0.1:18120', out = process.argv[2] ?? '../docs/img/manual'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.text())
await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
await api('POST', '/mount/connect', { driver: 'simulator' })
await api('PUT', '/site', { latitude: 40.0, longitude: -105.0 }).catch(() => {})
await new Promise((r) => setTimeout(r, 2000))
await api('POST', '/tracking/start', { mode: 'limb' }); await new Promise((r) => setTimeout(r, 2000))
const b = await chromium.launch()
const c = await b.newContext({ ...devices['iPhone 14'] }); const p = await c.newPage()
await p.goto(base); await p.waitForTimeout(1500)
const shot = (n) => p.screenshot({ path: `${out}/${n}.png` })
for (const [tab, name] of [['Camera', 'camera'], ['Aim', 'aim'], ['Track', 'track'], ['Record', 'record'], ['More', 'more']]) {
  await p.getByRole('tab', { name: new RegExp('^' + tab) }).click(); await p.waitForTimeout(700); await shot(name)
}
await p.getByRole('tab', { name: /^Aim/ }).click()
await p.getByText('Meridian flip').first().click().catch(() => {}); await p.waitForTimeout(400); await shot('meridian-flip')
await p.getByRole('button', { name: 'Hide controls' }).click(); await p.waitForTimeout(500); await shot('hide-controls')
await b.close()
