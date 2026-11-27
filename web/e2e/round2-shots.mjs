// Screenshots and layout numbers per viewport and tab: node e2e/round2-shots.mjs <out dir> [base url] [nocam]
// Needs a simulator server (see AGENTS.md). Prints panel/picture heights; used to compare before and after a layout change.
import { chromium, devices } from '@playwright/test'
const out = process.argv[2] ?? '.'
const base = process.argv[3] ?? 'http://127.0.0.1:18250'
const nocam = process.argv[4] === 'nocam'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
if (!nocam) {
  await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
  await api('POST', '/mount/connect', { driver: 'simulator' })
  await api('PUT', '/site', { latitude: 48.1, longitude: 11.6 })
}
const sizes = { phone: [390, 844], land: [844, 390], small: [667, 375], big: [932, 430], desk: [1280, 800] }
const only = (process.env.SIZES ?? 'phone,land,desk').split(',')
const tabs = (process.env.TABS ?? 'Camera,Aim,Track,Record,More').split(',')
const b = await chromium.launch()
for (const name of only) {
  const [w, h] = sizes[name]
  const touch = name !== 'desk'
  const c = await b.newContext({ viewport: { width: w, height: h }, hasTouch: touch, isMobile: touch, deviceScaleFactor: 2 })
  const p = await c.newPage()
  await p.goto(base)
  await p.waitForTimeout(1500)
  for (const t of tabs) {
    await p.getByRole('tab', { name: new RegExp('^' + t) }).click()
    await p.waitForTimeout(600)
    const m = await p.evaluate(() => {
      const r = (s) => { const e = document.querySelector(s); if (!e) return null; const b = e.getBoundingClientRect(); return [Math.round(b.x), Math.round(b.y), Math.round(b.width), Math.round(b.height)] }
      const panel = document.querySelector('.panel')
      return { live: r('.live') ?? r('.placeholder'), strip: r('.strip'), panel: r('.panel'), scroll: panel.scrollHeight, tabs: r('.tabs'), sbar: r('.sbar'), docW: document.documentElement.scrollWidth }
    })
    console.log(name, t, JSON.stringify(m))
    await p.screenshot({ path: `${out}/${name}-${t.toLowerCase()}.png` })
  }
  await c.close()
}
await b.close()
