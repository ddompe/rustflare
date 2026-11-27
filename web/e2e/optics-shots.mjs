// Screenshots of the optical setup page and dialog: node e2e/optics-shots.mjs <out dir> [base url]
// Env: SIZES=phone,se,small,desk  WIDE=1 (force Verdana)  THEME=day|night. Needs a simulator server (see AGENTS.md).
import { chromium } from '@playwright/test'
const out = process.argv[2] ?? '.'
const base = process.argv[3] ?? 'http://127.0.0.1:18290'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
await api('PUT', '/optics', { focal_length_mm: 400, focal_multiplier: 2, pixel_size_um: null, binning: 1 })
await api('PUT', '/optics/setups/' + encodeURIComponent('Lunt + Barlow'), { aperture_mm: 50, notes: 'H-alpha' })
await api('PUT', '/optics/setups/C11%20%2B%20reducer', { profile: 'moon', focal_length_mm: 2800, focal_multiplier: 0.63, activate: false })
const sizes = { phone: [390, 844], se: [375, 667], small: [667, 375], desk: [1280, 800] }
const only = (process.env.SIZES ?? 'phone,small,desk').split(',')
const b = await chromium.launch()
for (const name of only) {
  const [w, h] = sizes[name]
  const touch = name !== 'desk'
  const c = await b.newContext({ viewport: { width: w, height: h }, hasTouch: touch, isMobile: touch, deviceScaleFactor: 2 })
  const p = await c.newPage()
  await api('PUT', '/ui', { key: 'ui.theme', value: process.env.THEME ?? 'night' })
  await p.goto(base)
  if (process.env.WIDE) await p.addStyleTag({ content: '* { font-family: Verdana, DejaVu Sans !important }' })
  await p.waitForTimeout(800)
  await p.getByRole('tab', { name: /^More/ }).click()
  const d = p.locator('details.disc', { has: p.locator('.disc-title', { hasText: /^Optical setup$/ }) })
  if (!(await d.evaluate((e) => e.open))) await d.locator('summary').click()
  await d.scrollIntoViewIfNeeded()
  await p.waitForTimeout(400)
  await p.screenshot({ path: `${out}/${name}-page.png` })
  await p.getByRole('button', { name: 'Manage…' }).click()
  await p.waitForTimeout(300)
  await p.screenshot({ path: `${out}/${name}-list.png` })
  await p.getByRole('button', { name: 'New setup…' }).click()
  await p.getByRole('button', { name: 'Save', exact: true }).click()
  await p.waitForTimeout(300)
  await p.screenshot({ path: `${out}/${name}-form-error.png` })
  await p.getByLabel('Name', { exact: true }).fill('Test')
  await p.getByLabel('Aperture (mm)').fill('50')
  await p.waitForTimeout(200)
  await p.screenshot({ path: `${out}/${name}-form.png` })
  await c.close()
}
await b.close()
