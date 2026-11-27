// Layout numbers and screenshots per viewport, tab and theme: node e2e/round3-shots.mjs <out dir> [base url]
// Env: SIZES=phone,land,small,big,tiny,desk  TABS=Camera,Aim,...  WIDE=1 (force Verdana)  THEME=day|night  NOCAM=1
// Needs a simulator server (see AGENTS.md). Prints what the round 3 critique measured: picture, header against the tab bar, clipped tab labels, pad on screen.
import { chromium } from '@playwright/test'
const out = process.argv[2] ?? '.'
const base = process.argv[3] ?? 'http://127.0.0.1:18280'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
if (!process.env.NOCAM) {
  await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
  await api('POST', '/mount/connect', { driver: 'simulator' })
  await api('PUT', '/camera/control', { id: 'exposure_us', value: 2000 })
}
const sizes = { phone: [390, 844], se: [375, 667], land: [844, 390], small: [667, 375], big: [932, 430], tiny: [568, 320], desk: [1280, 800] }
const only = (process.env.SIZES ?? 'phone,land,small,tiny,desk').split(',')
const tabs = (process.env.TABS ?? 'Camera,Aim,Track,Record,More').split(',')
const theme = process.env.THEME ?? 'night'
const b = await chromium.launch()
for (const name of only) {
  const [w, h] = sizes[name]
  const touch = name !== 'desk'
  const c = await b.newContext({ viewport: { width: w, height: h }, hasTouch: touch, isMobile: touch, deviceScaleFactor: 2 })
  const p = await c.newPage()
  await api('PUT', '/ui', { key: 'ui.theme', value: theme })
  await p.goto(base)
  if (process.env.WIDE) await p.addStyleTag({ content: '* { font-family: Verdana !important }' })
  await p.waitForTimeout(1200)
  for (const t of tabs) {
    await p.getByRole('tab', { name: new RegExp('^' + t) }).click()
    await p.waitForTimeout(500)
    const m = await p.evaluate(() => {
      const R = (e) => { if (!e) return null; const b = e.getBoundingClientRect(); return [Math.round(b.x), Math.round(b.y), Math.round(b.width), Math.round(b.height)] }
      const q = (s) => document.querySelector(s)
      const tabsBox = q('.tabs').getBoundingClientRect()
      const head = [...q('header.top').children].map((e) => ({ c: e.className || e.tagName, r: R(e) }))
      const hb = q('.hidebtn')?.getBoundingClientRect()
      const hit = hb ? [0.1, 0.5, 0.9].map((f) => document.elementFromPoint(hb.x + hb.width * f, hb.y + hb.height / 2) === q('.hidebtn') || q('.hidebtn').contains(document.elementFromPoint(hb.x + hb.width * f, hb.y + hb.height / 2))) : null
      const hov = [...q('header.top').querySelectorAll('*')].filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.x < tabsBox.right && r.right > tabsBox.x && r.y < tabsBox.bottom && r.bottom > tabsBox.y }).length
      const clipped = [...document.querySelectorAll('.tabs button > span:not(.sr-only)')].filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent)
      const pad = [...document.querySelectorAll('.pad button')].map((e) => { const r = e.getBoundingClientRect(); const pr = q('.panel').getBoundingClientRect(); return r.bottom <= pr.bottom + 0.5 && r.top >= pr.top - 0.5 && r.bottom <= innerHeight + 0.5 })
      return { live: R(q('.live') ?? q('.placeholder')), strip: R(q('.strip')), panel: R(q('.panel')), tabs: R(q('.tabs')), top: R(q('header.top')), head, hit, headOverTabs: hov, clipped, padOn: pad.length ? pad.every(Boolean) : null, docW: document.documentElement.scrollWidth, panelScroll: q('.panel').scrollHeight }
    })
    console.log(name, t, JSON.stringify(m))
    await p.screenshot({ path: `${out}/${name}-${theme}-${t.toLowerCase()}${process.env.WIDE ? '-wide' : ''}.png` })
  }
  await c.close()
}
await b.close()
