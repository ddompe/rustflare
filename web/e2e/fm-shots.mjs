// Screenshots of the file manager for a design review: node e2e/fm-shots.mjs <base url> <out dir>
import { chromium, devices } from '@playwright/test'
const base = process.argv[2], out = process.argv[3]
const b = await chromium.launch()
const shots = async (label, device, steps) => {
  const c = await b.newContext({ ...device, ignoreHTTPSErrors: true }); const p = await c.newPage()
  await p.goto(base); await p.waitForTimeout(1500)
  await p.getByRole('tab', { name: /^Record/ }).click()
  await p.getByRole('button', { name: /Open file manager/ }).click(); await p.waitForTimeout(800)
  let i = 1
  for (const [name, fn] of steps) { await fn(p); await p.waitForTimeout(1500); await p.screenshot({ path: `${out}/${label}-${String(i++).padStart(2, '0')}-${name}.png` }) }
  await c.close()
}
const fm = (p) => p.getByRole('dialog', { name: 'File manager' })
const row = (p, t) => fm(p).locator(`.fm-row[data-type="${t}"]`).first().locator('.fm-open')
const steps = [
  ['top', async () => {}],
  ['stacks', async (p) => row(p, 'stacks').click()],
  ['stacks-day', async (p) => row(p, 'folder').click()],
  ['session-folder', async (p) => row(p, 'prominence').click()],
  ['lane', async (p) => row(p, 'lane').click()],
]
await shots('phone', devices['iPhone 14'], steps)
await shots('desk', { viewport: { width: 1280, height: 820 } }, steps)
await b.close()
