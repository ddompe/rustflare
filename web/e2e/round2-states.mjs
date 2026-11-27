// Screenshots of particular states for a visual check: node e2e/round2-states.mjs <out dir> [base url]
import { chromium } from '@playwright/test'
const out = process.argv[2] ?? '.'
const base = process.argv[3] ?? 'http://127.0.0.1:18250'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
await api('POST', '/camera/open', { driver: 'simulator', id: 'sim_osc' })
await api('POST', '/mount/connect', { driver: 'simulator' })
await api('PUT', '/site', { latitude: 48.1, longitude: 11.6 })
await api('PUT', '/profile', { id: 'planet' })
await api('POST', '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 48.1, longitude_east_deg: 11.6 })
await api('PUT', '/mount/auto-site-time', { enabled: false })
const b = await chromium.launch()
const shot = async (name, w, h, steps) => {
  const c = await b.newContext({ viewport: { width: w, height: h }, hasTouch: w < 900, isMobile: w < 900, deviceScaleFactor: 2 })
  const p = await c.newPage()
  await p.goto(base); await p.waitForTimeout(1500)
  for (const [n, fn] of steps) { await fn(p); await p.waitForTimeout(500); await p.screenshot({ path: `${out}/${name}-${n}.png` }) }
  await c.close()
}
const tab = (t) => (p) => p.getByRole('tab', { name: new RegExp('^' + t) }).click()
await shot('p', 390, 844, [
  ['camera-notes', async (p) => { await tab('Camera')(p); await p.getByRole('button', { name: /\d notes?$/ }).click() }],
  ['track-planet', tab('Track')],
  ['record-planet', tab('Record')],
  ['aim-clock', async (p) => { await tab('Aim')(p); await p.locator('details.disc summary', { hasText: 'Mount time' }).click() }],
  ['more-optics', async (p) => { await tab('More')(p); await p.locator('details.disc summary', { hasText: 'Optical setup' }).click() }],
  ['hide', async (p) => { await p.getByRole('button', { name: 'Hide controls' }).click() }],
])
await shot('l', 844, 390, [
  ['camera', tab('Camera')],
  ['aim', tab('Aim')],
  ['record', tab('Record')],
  ['hide', async (p) => { await p.getByRole('button', { name: 'Hide controls' }).click() }],
])
await shot('d', 1280, 800, [
  ['camera-day', async (p) => { await p.getByRole('button', { name: /daylight/ }).click(); await tab('Camera')(p) }],
  ['hide', async (p) => { await p.getByRole('button', { name: 'Hide controls' }).click() }],
])
await api('POST', '/camera/close')
await shot('p', 390, 844, [['nocam', tab('Camera')]])
await b.close()
