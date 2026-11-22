// Screenshots of the states that matter, for a design review: node e2e/ui-review-shots.mjs <out dir> (server on :18121)
import { chromium, devices } from '@playwright/test'
const base = 'http://127.0.0.1:18121', out = process.argv[2] ?? '../.local/ui-review'
const api = (m, p, b) => fetch(base + '/api' + p, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined }).then((r) => r.json().catch(() => ({})))
const wait = (ms) => new Promise((r) => setTimeout(r, ms))
await api('POST', '/camera/open', { driver: 'simulator', id: 'sim0' })
await api('POST', '/mount/connect', { driver: 'simulator' })
await api('PUT', '/camera/control', { id: 'sim_zoom', value: 0.9 })
// a site where the Sun crosses the meridian in about 4 minutes
await api('PUT', '/site', { latitude: 40, longitude: 0 })
const m0 = (await (await fetch(base + '/api/status')).json()).sky_sun.minutes_to_meridian
await api('PUT', '/site', { latitude: 40, longitude: (m0 - 4) / 4 })
await api('POST', '/mount/tracking', { enabled: true }); await wait(1500)
await api('POST', '/tracking/start', { mode: 'limb' }); await wait(1500)
const b = await chromium.launch()
const shots = async (label, device, steps) => {
  const c = await b.newContext(device); const p = await c.newPage()
  await p.goto(base); await p.waitForTimeout(1500)
  for (const [name, fn] of steps) { await fn(p); await p.waitForTimeout(700); await p.screenshot({ path: `${out}/${label}-${name}.png` }) }
  await c.close()
}
const tab = (t) => (p) => p.getByRole('tab', { name: new RegExp('^' + t) }).click()
const phone = devices['iPhone 14'], land = devices['iPhone 14 landscape'], desk = { viewport: { width: 1280, height: 820 } }
await api('PUT', '/mount/autoflip', { enabled: true, record_min: 5, flip_min: 1 })
await shots('phone', phone, [
  ['01-camera', tab('Camera')],
  ['02-aim-flip-armed', async (p) => { await tab('Aim')(p); await p.getByText('Meridian flip').first().click().catch(() => {}) }],
  ['03-track', tab('Track')],
])
await api('PUT', '/coronagraph', { enabled: true, scale: 1.03 }); await api('PUT', '/focus', { enabled: true }); await wait(2500)
await shots('phone', phone, [['04-camera-mask-focus', tab('Camera')]])
await shots('desk', desk, [['01-camera-mask-focus', tab('Camera')], ['02-aim', tab('Aim')]])
await api('PUT', '/coronagraph', { enabled: false, scale: 1.03 }); await api('PUT', '/focus', { enabled: false })
await api('PUT', '/camera/control', { id: 'sim_zoom', value: 8 }); await wait(5000)
await shots('phone', phone, [['05-track-lost', tab('Track')], ['06-camera-lost', tab('Camera')]])
await api('PUT', '/camera/control', { id: 'sim_zoom', value: 0.9 }); await wait(2500)
await api('POST', '/capture/lanes', { name: 'rv', cycles: 50, cycle_interval_s: 5, format: 'ser', lanes: [{ name: 'disc', exposure_us: 2000, frames: 10 }, { name: 'prom', exposure_us: 20000, frames: 5 }] }); await wait(2500)
await shots('phone', phone, [['07-record-recording', tab('Record')], ['08-camera-recording', tab('Camera')]])
await api('POST', '/capture/pause'); await wait(1500)
await shots('phone', phone, [['09-record-paused', tab('Record')], ['10-camera-paused', tab('Camera')]])
await shots('land', land, [['01-camera-paused', tab('Camera')]])
await api('POST', '/capture/stop')
await b.close()
