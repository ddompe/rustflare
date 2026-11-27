// States the layout critique looked at, as screenshots: node e2e/round3-states.mjs <out dir> [base url]
// Env: WIDE=1 forces a wide font, THEME=day|night, SIZES=phone,small,... (sizes as in round3-shots.mjs)
import { chromium } from '@playwright/test'
const out = process.argv[2] ?? '.'
const base = process.argv[3] ?? 'http://127.0.0.1:18280'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
await api('POST', '/mount/connect', { driver: 'simulator' })
await api('PUT', '/camera/control', { id: 'exposure_us', value: 2000 })
await api('PUT', '/site', { latitude: 48.1, longitude: 11.6 })
await api('POST', '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 48.1, longitude_east_deg: 11.6 })
await api('PUT', '/mount/auto-site-time', { enabled: false })
await api('PUT', '/ui', { key: 'ui.theme', value: process.env.THEME ?? 'night' })
await api('PUT', '/ui', { key: 'ui.setup_dismissed', value: false })
const sizes = { phone: [390, 844], se: [375, 667], land: [844, 390], small: [667, 375], tiny: [568, 320], desk: [1280, 800] }
const only = (process.env.SIZES ?? 'phone,small').split(',')
const b = await chromium.launch()
for (const name of only) {
  const [w, h] = sizes[name]
  const touch = name !== 'desk'
  const c = await b.newContext({ viewport: { width: w, height: h }, hasTouch: touch, isMobile: touch, deviceScaleFactor: 2 })
  const p = await c.newPage()
  await p.goto(base)
  if (process.env.WIDE) await p.addStyleTag({ content: '* { font-family: Verdana, "DejaVu Sans", sans-serif !important; letter-spacing: .02em !important }' })
  await p.waitForTimeout(1200)
  const shot = (n) => p.screenshot({ path: `${out}/${name}-${n}${process.env.WIDE ? '-wide' : ''}-${process.env.THEME ?? 'night'}.png` })
  await p.getByRole('tab', { name: /^Aim/ }).click()
  await p.waitForTimeout(400)
  await p.getByRole('button', { name: 'Picture tools' }).click()
  await p.waitForTimeout(300)
  await shot('aim-tools')
  await p.keyboard.press('Escape')
  await p.mouse.click(5, 5)
  const chip = p.locator('.chip.notes')
  if (await chip.count()) { await chip.click(); await p.waitForTimeout(300); await shot('aim-notes'); await p.keyboard.press('Escape') }
  await p.getByRole('tab', { name: /^Track/ }).click()
  await p.waitForTimeout(300)
  await shot('track')
  await p.getByRole('tab', { name: /^Camera/ }).click()
  await p.waitForTimeout(300)
  await shot('camera')
  await p.getByRole('tab', { name: /^More/ }).click()
  await p.locator('summary', { hasText: 'Site' }).click()
  await p.waitForTimeout(300)
  await shot('more-site')
  await c.close()
}
await b.close()
