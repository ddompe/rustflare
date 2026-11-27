import { expect, Page, test } from '@playwright/test'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })
const status = async (page: Page) => (await page.request.get('/api/status')).json()
const tab = (page: Page, t: string) => page.getByRole('tab', { name: new RegExp('^' + t) }).click()
const overlap = (a: { x: number; y: number; width: number; height: number }, b: { x: number; y: number; width: number; height: number }) =>
  Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)) * Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y))

/** A wide fallback font (the CI machine has a wider one than a Mac): Verdana where it exists, DejaVu Sans on Linux, plus letter spacing so the effect never depends on which exists. */
const WIDE = '* { font-family: Verdana, "DejaVu Sans", sans-serif !important; letter-spacing: .02em !important }'

/** Rewrites the status the page receives over its WebSocket, to put the UI in a state the simulators do not reach by themselves. Call before page.goto. */
async function patchStatus(page: Page, patch: (s: any) => void) { // eslint-disable-line @typescript-eslint/no-explicit-any
  await page.routeWebSocket(/\/api\/ws/, (ws) => {
    const server = ws.connectToServer()
    server.onMessage((m) => { try { const s = JSON.parse(String(m)); patch(s); ws.send(JSON.stringify(s)) } catch { ws.send(m) } })
    ws.onMessage((m) => server.send(m))
  })
}

test.afterEach(async ({ page }) => { await post(page, '/capture/stop') })

test.beforeEach(async ({ page }, info) => {
  test.skip(info.project.name !== 'phone', 'sizes are set in each test; one project is enough')
  await put(page, '/storage/reserve', { gb: 2 })
  await post(page, '/camera/close'); await post(page, '/mount/disconnect')
  await post(page, '/camera/open', { driver: 'simulator', id: 'sim0' })
  await post(page, '/mount/connect', { driver: 'simulator' })
  await put(page, '/profile', { id: 'sun_ha' })
  await put(page, '/site', { latitude: 40, longitude: -105 })
  await post(page, '/debug/sim-mount-clock', { time_offset_s: 0, latitude_deg: 40, longitude_east_deg: -105 })
  for (const [key, value] of [['ui.setup_dismissed', false], ['ui.profile_confirmed', false], ['ui.theme', 'night'], ['ui.autolock_seen', false], ['tracking.mode', '']] as const) await put(page, '/ui', { key, value })
  await expect.poll(async () => (await status(page)).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
})

const LANDSCAPES = [{ w: 667, h: 375 }, { w: 568, h: 320 }, { w: 932, h: 430 }, { w: 390, h: 844 }, { w: 375, h: 667 }]

test.describe('P1a: the header never covers the tab bar', () => {
  for (const v of LANDSCAPES) for (const wide of [false, true]) {
    test(`${v.w}x${v.h}${wide ? ' with a wide font' : ''}: Files, theme and Hide controls are whole and tappable, the tabs are not clipped`, async ({ page }) => {
      await page.setViewportSize({ width: v.w, height: v.h })
      await page.goto('/')
      if (wide) await page.addStyleTag({ content: WIDE })
      await expect(page.locator('.live')).toBeVisible()
      const bad = await page.evaluate(() => {
        const tabs = document.querySelector('.tabs')!.getBoundingClientRect()
        const hit = (e: Element) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && r.left < tabs.right && r.right > tabs.left && r.top < tabs.bottom && r.bottom > tabs.top }
        return [...document.querySelectorAll('header.top *')].filter(hit).map((e) => (e.className && String(e.className)) || e.tagName)
      })
      expect(bad, 'header elements over the tab bar').toEqual([])
      const top = (await page.locator('header.top').boundingBox())!
      for (const name of ['Files', 'Hide controls']) {
        const b = (await page.getByRole('button', { name: new RegExp('^' + name) }).boundingBox())!
        expect(b.height, `${name} height`).toBeGreaterThanOrEqual(43.5)
        expect(b.x + b.width, `${name} inside the header`).toBeLessThanOrEqual(top.x + top.width + 0.5)
        expect(b.x + b.width, `${name} on the screen`).toBeLessThanOrEqual(v.w)
      }
      const hide = page.getByRole('button', { name: 'Hide controls' })
      const hb = (await hide.boundingBox())!
      for (const f of [0.1, 0.5, 0.9]) {
        const ok = await page.evaluate(({ x, y }) => { const e = document.elementFromPoint(x, y); return !!e && !!e.closest('.hidebtn') }, { x: hb.x + hb.width * f, y: hb.y + hb.height / 2 })
        expect(ok, `Hide controls hit-tests to itself at ${f * 100}%`).toBe(true)
      }
      const theme = (await page.locator('.themebtn').boundingBox())!
      expect(theme.width).toBeGreaterThanOrEqual(43.5)
      const labels = await page.locator('.tabs button > span:not(.sr-only)').evaluateAll((els) => els.map((e) => ({ text: e.textContent, clipped: e.scrollWidth > e.clientWidth + 1 })))
      expect(labels.map((l) => l.text)).toEqual(['Camera', 'Aim', 'Track', 'Record', 'More'])
      expect(labels.filter((l) => l.clipped), 'clipped tab labels').toEqual([])
      expect(await page.evaluate(() => document.documentElement.scrollWidth), 'page scrolls sideways').toBeLessThanOrEqual(v.w)
      await hide.click({ position: { x: Math.max(2, hb.width * 0.9), y: hb.height / 2 } })
      await expect(page.getByRole('button', { name: 'Show controls' })).toBeVisible()
      await page.getByRole('button', { name: 'Show controls' }).click()
      await expect(hide).toBeVisible()
    })
  }
})

test.describe('P1b: the Aim picture keeps its size', () => {
  for (const v of [{ w: 390, h: 844 }, { w: 375, h: 667 }]) for (const wide of [false, true]) {
    test(`${v.w}x${v.h}${wide ? ' wide font' : ''}: constant while notes come and go, as big as the width allows, the whole pad on screen`, async ({ page }) => {
      await put(page, '/mount/auto-site-time', { enabled: false })
      await page.setViewportSize({ width: v.w, height: v.h })
      await page.goto('/')
      if (wide) await page.addStyleTag({ content: WIDE })
      await tab(page, 'Aim')
      await expect(page.locator('.live')).toBeVisible()
      await expect(page.locator('.pad')).toBeVisible()
      const live = async () => (await page.locator('.live').boundingBox())!
      const before = await live()
      const s = await status(page)
      const ar = s.camera.width / s.camera.height
      expect(before.height, 'picture height: at least 40% of the screen, or the full width of a 4:3 frame').toBeGreaterThanOrEqual(Math.min(v.h * 0.4, v.w / ar) - 2)
      // a mount clock that differs brings a note; sending it clears it
      await post(page, '/debug/sim-mount-clock', { time_offset_s: 600, latitude_deg: 40, longitude_east_deg: -105 })
      await expect(page.locator('.chip.notes')).toBeVisible()
      const during = await live()
      expect(during.height, 'picture height with a note').toBeCloseTo(before.height, 0)
      expect(during.width, 'picture width with a note').toBeCloseTo(before.width, 0)
      await post(page, '/mount/sync-site-time')
      await expect(page.locator('.chip.notes')).toHaveCount(0)
      const after = await live()
      expect(after.height, 'picture height after the note clears').toBeCloseTo(before.height, 0)
      // the five pad buttons are inside the visible panel, no scrolling
      const panel = (await page.locator('.panel').boundingBox())!
      for (const b of await page.locator('.pad button').all()) {
        const r = (await b.boundingBox())!
        expect(r.y, 'pad button top').toBeGreaterThanOrEqual(panel.y - 0.5)
        expect(r.y + r.height, 'pad button bottom').toBeLessThanOrEqual(panel.y + panel.height + 0.5)
      }
      await put(page, '/mount/auto-site-time', { enabled: true })
    })
  }
})

test.describe('P2a: set up N of 5', () => {
  test('counts only what is confirmed, sits under the Start strip on one line, and stays as a chip on the other tabs', async ({ page }) => {
    await post(page, '/mount/disconnect')
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Camera')
    const strip = page.getByRole('navigation', { name: 'First session set up' })
    const s = await status(page)
    const done = (s.camera ? 1 : 0) + 0 /* profile not confirmed */ + (s.optics?.focal_length_mm != null || s.optics?.setup?.active ? 1 : 0) + (s.site ? 1 : 0) + 0 /* no mount */
    await expect(strip).toContainText(`Set up ${done} of 5`)
    await expect(strip.getByRole('button', { name: /Connect the mount/ })).toBeVisible()
    await expect(strip.getByRole('button', { name: /^Keep / })).toBeVisible()
    const sb = (await strip.boundingBox())!
    const start = (await page.locator('.strip').boundingBox())!
    expect(sb.y, 'right under the Start strip').toBeGreaterThanOrEqual(start.y + start.height - 1)
    expect(sb.y + sb.height, 'on the first screen').toBeLessThan(844 - 100)
    expect(sb.height, 'one line').toBeLessThanOrEqual(64)
    // the x is on the same line as the label
    const x = (await strip.getByRole('button', { name: 'Hide the set up steps' }).boundingBox())!
    const label = (await strip.locator('.ss-label').boundingBox())!
    expect(Math.abs(x.y + x.height / 2 - (label.y + label.height / 2))).toBeLessThan(20)
    // confirming the profile counts
    await page.getByRole('button', { name: /^Keep / }).click()
    await expect(strip).toContainText(`Set up ${done + 1} of 5`)
    // on the other tabs it is one chip that opens the same steps
    await tab(page, 'Track')
    const chip = page.getByRole('button', { name: `Set up ${done + 1} of 5` })
    await expect(chip).toBeVisible()
    await chip.click()
    await expect(page.getByRole('region', { name: 'Set up steps' }).getByRole('button', { name: /Connect the mount/ })).toBeVisible()
    await page.getByRole('region', { name: 'Set up steps' }).getByRole('button', { name: /Connect the mount/ }).click()
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('aria-selected', 'true')
  })

  test('a connected mount with a matching clock counts; a differing one does not', async ({ page }) => {
    await put(page, '/ui', { key: 'ui.profile_confirmed', value: true })
    await put(page, '/mount/auto-site-time', { enabled: false })
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 600, latitude_deg: 40, longitude_east_deg: -105 })
    await page.goto('/')
    await tab(page, 'Camera')
    const strip = page.getByRole('navigation', { name: 'First session set up' })
    await expect(strip.getByRole('button', { name: /Mount time/ })).toBeVisible()
    await post(page, '/mount/sync-site-time')
    await expect(strip.getByRole('button', { name: /Mount time/ })).toHaveCount(0)
    await put(page, '/mount/auto-site-time', { enabled: true })
  })
})

test.describe('P2b: location needs a secure page', () => {
  const insecure = (page: Page) => page.addInitScript(() => { Object.defineProperty(window, 'isSecureContext', { value: false, configurable: true }) })

  test('on plain http Type it in is the main action, the location button is not offered, and nothing pops up a native alert', async ({ page }) => {
    await insecure(page)
    await patchStatus(page, (s) => { s.site = null })
    let dialog = false
    page.on('dialog', () => { dialog = true })
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Aim')
    const needs = page.locator('#why-goto-site')
    await expect(needs).toContainText('Set your site first')
    await expect(needs.getByRole('button', { name: 'Use my location' })).toHaveCount(0)
    await expect(needs.getByRole('button', { name: 'Type it in' })).toHaveClass(/primary/)
    await expect(needs).toContainText('needs HTTPS')
    await needs.getByRole('button', { name: 'Type it in' }).click()
    await expect(page.getByRole('tab', { name: /^More/ })).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('button', { name: "Use this device's location" })).toHaveCount(0)
    await expect(page.getByText(/needs HTTPS, and this page is plain http/).first()).toBeVisible()
    expect(dialog).toBe(false)
  })

  test('on a secure page the location button is the main action', async ({ page }) => {
    await patchStatus(page, (s) => { s.site = null })
    await page.goto('/')
    await tab(page, 'Aim')
    await expect(page.locator('#why-goto-site').getByRole('button', { name: 'Use my location' })).toHaveClass(/primary/)
  })

  test('a refused location says what to do, in the block that asked', async ({ page }) => {
    await page.addInitScript(() => { Object.defineProperty(navigator, 'geolocation', { value: { getCurrentPosition: (_ok: unknown, bad: (e: { message: string }) => void) => bad({ message: 'User denied Geolocation' }) }, configurable: true }) })
    await patchStatus(page, (s) => { s.site = null })
    await page.goto('/')
    await tab(page, 'Aim')
    await page.locator('#why-goto-site').getByRole('button', { name: 'Use my location' }).click()
    await expect(page.locator('#why-goto-site').or(page.locator('.home')).getByRole('alert').first()).toContainText(/Allow location|type the latitude/i)
  })
})

test.describe('P2c: failures say what to do', () => {
  const CAL_FAILED = 'calibration failed: axis directions are parallel; calibration unusable'
  const tracking = (s: any) => { s.tracking = { mode: 'limb', x: 640, y: 480, radius: 300, lost: false }; s.mount.tracking = true } // eslint-disable-line @typescript-eslint/no-explicit-any

  test('a failed calibration is a state of the Calibrate row, with Retry, and the status bar shows all of it', async ({ page }) => {
    await patchStatus(page, (s) => { tracking(s); s.guiding.state = 'off'; s.guiding.calibration = null; s.error = CAL_FAILED })
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Track')
    const row = page.locator('li.step', { hasText: 'Calibrate' })
    await expect(row).toContainText('failed')
    await expect(row).toContainText('the picture did not move along two different directions')
    await expect(row).not.toContainText('needed once')
    await expect(row.getByRole('button', { name: 'Retry' })).toBeEnabled()
    await expect(page.getByRole('tab', { name: /^Track/ })).toHaveAttribute('title', /calibration failed/)
    // the bar says it in full when opened, and its closed form wraps (two lines) instead of cutting after a few words
    const bar = page.locator('.sbar-btn')
    await bar.click()
    await expect(page.locator('.sbar-pop')).toContainText('Check that the Sun is in view and the mount is tracking, then Calibrate again.')
    const pop = (await page.locator('.sbar-pop').boundingBox())!
    expect(pop.width).toBeGreaterThan(300)
  })

  test('while calibrating there is a way to stop it', async ({ page }) => {
    await patchStatus(page, (s) => { tracking(s); s.guiding.state = 'calibrating' })
    await page.goto('/')
    await tab(page, 'Track')
    const stop = page.getByRole('button', { name: 'Stop calibration' })
    await expect(stop).toBeVisible()
    const req = page.waitForRequest((r) => r.url().endsWith('/api/guiding/stop') && r.method() === 'POST')
    await stop.click()
    await req
  })

  test('off target says what to do and the Track tab is not green; locked is', async ({ page }) => {
    await patchStatus(page, (s) => { tracking(s); s.guiding.state = 'guiding'; s.guiding.calibration = { primary_dir: { x: 1, y: 0 }, secondary_dir: { x: 0, y: 1 } }; s.guiding.error_px = 165.4 })
    await page.goto('/')
    await tab(page, 'Track')
    await expect(page.getByRole('alert').filter({ hasText: 'Off target by 165 px' })).toContainText('recalibrate')
    await expect(page.getByRole('tab', { name: /^Track/ })).toHaveAttribute('title', /needs attention, off target by 165 px/)
    await expect(page.getByRole('tab', { name: /^Track/ }).locator('.tone-icon.t-ok')).toHaveCount(0)
  })

  test('locked is the one state where the Track tab is ok', async ({ page }) => {
    await patchStatus(page, (s) => { tracking(s); s.guiding.state = 'guiding'; s.guiding.calibration = { primary_dir: { x: 1, y: 0 }, secondary_dir: { x: 0, y: 1 } }; s.guiding.error_px = 3 })
    await page.goto('/')
    await expect(page.getByRole('tab', { name: /^Track/ })).toHaveAttribute('title', /ready, holding the target/)
  })

  test('a recalibrate that is off says why', async ({ page }) => {
    await patchStatus(page, (s) => { tracking(s); s.guiding.state = 'guiding'; s.guiding.calibration = { primary_dir: { x: 1, y: 0 }, secondary_dir: { x: 0, y: 1 } }; s.guiding.error_px = 3 })
    await page.goto('/')
    await tab(page, 'Track')
    const b = page.getByRole('button', { name: 'Recalibrate' })
    await expect(b).toBeDisabled()
    await expect(b).toHaveAttribute('aria-describedby', 'why-cal')
    await expect(page.locator('#why-cal')).toContainText('Stop guiding')
  })

  test('a refusal from the server carries what to do next', async ({ page }) => {
    await post(page, '/capture/sequence', { frames: 600, format: 'ser' })
    await expect.poll(async () => (await status(page)).capture.state).not.toBe('idle')
    // the page believes nothing is running, so it offers Start; the engine refuses, and the page adds the hint
    await patchStatus(page, (s) => { s.capture.state = 'idle'; s.capture.paused = false })
    await page.goto('/')
    await page.getByRole('button', { name: /^Start/ }).click()
    await expect(page.locator('.strip .err')).toContainText('Stop it first (Record tab)')
    await post(page, '/capture/stop')
  })

  test('Close camera asks first while a recording runs, says what it ends, and closes at once when idle', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Camera')
    await post(page, '/capture/sequence', { frames: 600, format: 'ser' })
    await expect.poll(async () => (await status(page)).capture.state).not.toBe('idle')
    await expect(page.locator('.recbar')).toBeVisible()
    await page.getByRole('button', { name: 'Close camera' }).click()
    const ask = page.getByRole('alertdialog', { name: 'Close the camera' })
    await expect(ask).toContainText('the recording')
    expect((await status(page)).camera).not.toBeNull()
    await ask.getByRole('button', { name: 'Keep the camera open' }).click()
    await expect(ask).toHaveCount(0)
    expect((await status(page)).camera).not.toBeNull()
    await page.getByRole('button', { name: 'Close camera' }).click()
    await ask.getByRole('button', { name: 'Close the camera' }).click()
    await expect.poll(async () => (await status(page)).camera).toBeNull()
    // idle: no question
    await post(page, '/camera/open', { driver: 'simulator', id: 'sim0' })
    await expect(page.getByRole('button', { name: 'Close camera' })).toBeVisible()
    await page.getByRole('button', { name: 'Close camera' }).click()
    await expect.poll(async () => (await status(page)).camera).toBeNull()
  })

  test('Track and Record without a camera have a button that goes to Camera', async ({ page }) => {
    await post(page, '/camera/close')
    await page.goto('/')
    await tab(page, 'Track')
    await page.getByRole('button', { name: /Open the camera/ }).click()
    await expect(page.getByRole('tab', { name: /^Camera/ })).toHaveAttribute('aria-selected', 'true')
    await tab(page, 'Record')
    await page.getByRole('button', { name: /Open the camera/ }).click()
    await expect(page.getByRole('tab', { name: /^Camera/ })).toHaveAttribute('aria-selected', 'true')
  })
})

test.describe('P2d: tab marks and the notes chip agree with the content', () => {
  const diffClock = (s: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
    const place = { latitude_deg: 40, longitude_east_deg: -105, unix_secs: 1_700_000_000, utc_offset_minutes: 0 }
    s.mount.site_time = { read_at_unix: 1_700_000_000, mount: { ...place, unix_secs: 1_700_000_180 }, pi: place, time_diff_s: 180, place_diff_deg: 0, time_ok: false, place_ok: true, ok: false, why: 'x' }
  }

  test('Aim is a warning while the mount clock differs, the chip says so in its first words', async ({ page }) => {
    await patchStatus(page, (s) => { diffClock(s); s.mount.tracking = true })
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('title', /needs attention, the mount clock or place differs/)
    await expect(page.getByRole('tab', { name: /^Aim/ }).locator('.tone-icon.t-ok')).toHaveCount(0)
    const chip = page.locator('.chip.notes')
    await expect(chip).toHaveClass(/warn/)
    await expect(chip).toContainText('Clock off 3 min')
  })

  test('with two notes the chip lists them, and the profile reminder is not one of them', async ({ page }) => {
    await put(page, '/profile', { id: 'planet' })
    await patchStatus(page, (s) => { diffClock(s); s.sun = null; s.tracking = null })
    await page.goto('/')
    const chip = page.locator('.chip.notes')
    await expect(chip).toContainText(/^2 notes: /)
    await expect(chip).toContainText('clock')
    await chip.click()
    await expect(page.getByRole('region', { name: 'Notes' })).not.toContainText('Imaging:')
    await put(page, '/profile', { id: 'sun_ha' })
  })

  test('Aim is ok only when the mount is connected, tracking and its clock matches', async ({ page }) => {
    await patchStatus(page, (s) => { s.mount.tracking = true; if (s.mount.site_time) { s.mount.site_time.ok = true; s.mount.site_time.time_ok = true; s.mount.site_time.place_ok = true } })
    await page.goto('/')
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('title', /ready, mount connected and tracking/)
  })
})

test.describe('P3a: the picture tools say what they are', () => {
  test('the sheet names every tool, and on a phone it is not over the middle of the picture', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Aim')
    await page.getByRole('button', { name: 'Picture tools' }).click()
    const sheet = page.getByRole('group', { name: 'Picture tools' })
    for (const name of ['Zoom in', 'Zoom out', 'Prominence', 'Stretch', 'Overlays', 'Widgets', 'Display']) await expect(sheet.getByRole('button', { name })).toBeVisible()
    // the label is visible text, not only a tooltip
    for (const name of ['Zoom in', 'Stretch']) await expect(sheet.locator('button', { hasText: name })).toHaveText(name)
    const live = (await page.locator('.live').boundingBox())!
    const sb = (await sheet.boundingBox())!
    const centre = { x: live.x + live.width / 2, y: live.y + live.height / 2 }
    const covers = centre.x >= sb.x && centre.x <= sb.x + sb.width && centre.y >= sb.y && centre.y <= sb.y + sb.height
    expect(covers, 'the tools sheet covers the centre of the picture').toBe(false)
    expect(overlap(sb, live), 'the tools sheet over the picture').toBe(0)
    // the buttons that do not apply say why
    const zoomOut = sheet.getByRole('button', { name: 'Zoom out' })
    await expect(zoomOut).toBeDisabled()
    await expect(zoomOut).toHaveAttribute('aria-describedby', 'why-zoomout')
    await expect(page.locator('#why-zoomout')).toBeVisible()
    await sheet.getByRole('button', { name: 'Zoom in' }).click()
    await expect(zoomOut).toBeEnabled()
    await sheet.getByRole('button', { name: 'Widgets' }).click()
    await expect(page.getByRole('dialog', { name: 'Overlay widgets' })).toBeVisible()
  })

  for (const v of [{ w: 844, h: 390 }, { w: 1280, h: 800 }]) {
    test(`${v.w}x${v.h}: the same sheet, labelled`, async ({ page }) => {
      await page.setViewportSize({ width: v.w, height: v.h })
      await page.goto('/')
      await page.getByRole('button', { name: 'Picture tools' }).click()
      const sheet = page.getByRole('group', { name: 'Picture tools' })
      for (const name of ['Zoom in', 'Zoom out', 'Prominence', 'Stretch', 'Overlays', 'Widgets', 'Display']) await expect(sheet.getByRole('button', { name })).toBeVisible()
      const live = (await page.locator('.live').boundingBox())!
      const sb = (await sheet.boundingBox())!
      expect(sb.x + sb.width).toBeLessThanOrEqual(v.w + 0.5)
      expect(sb.y + sb.height).toBeLessThanOrEqual(v.h + 0.5)
      if (v.w < 900) expect(overlap(sb, live), 'sheet over the picture').toBe(0)
    })
  }
})

test.describe('P3b: Saved, UTC and the progress card', () => {
  test('after a burst the Record tab and the notes say what was saved, with its size and the way to it', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Record')
    await expect(page.getByText('Stops at whichever comes first')).toBeVisible()
    await post(page, '/capture/sequence', { frames: 20, format: 'ser' })
    const progress = page.locator('.progress')
    await expect(progress).toBeVisible()
    // one line says what it is doing; the buffer bar is named
    await expect(progress.locator('.prog-title')).toContainText('Recording')
    expect(await progress.locator('text=/burst 1\\/1: capturing/i').count()).toBe(0)
    if (await progress.locator('progress.buf').count()) await expect(progress.locator('progress.buf')).toHaveAttribute('aria-label', 'RAM buffer')
    await expect.poll(async () => (await status(page)).capture.state, { timeout: 25_000 }).toBe('idle')
    const line = page.locator('.saved-line')
    await expect(line).toContainText('Saved')
    await expect(line).toContainText(/\.ser/)
    await expect(line).toContainText(/\d+(\.\d)? (MB|GB)/)
    // on another tab it is among the notes
    await tab(page, 'Camera')
    const chip = page.locator('.chip.notes')
    await expect(chip).toContainText('Saved')
    await chip.click()
    await page.getByRole('region', { name: 'Notes' }).getByRole('button', { name: 'Open in Files' }).click()
    await expect(page.getByRole('heading', { name: 'Files' })).toBeVisible()
    // the UTC names are explained, and times are in the device's zone, named
    await expect(page.locator('.fm-sub', { hasText: 'named in UTC' }).first()).toContainText(/\d\d:\d\d:\d\d UTC/)
  })
})

test.describe('P3c: one name for each thing', () => {
  const walk = (dir: string): string[] => readdirSync(dir).flatMap((f) => { const p = join(dir, f); return statSync(p).isDirectory() ? walk(p) : p.endsWith('.tsx') || p.endsWith('.ts') ? [p] : [] })
  const src = () => walk(join(process.cwd(), 'src')).map((p) => ({ p, s: readFileSync(p, 'utf8') }))

  test('the retired phrases are gone from the source', () => {
    const banned = ['Optical setup in use', 'Optical setup of this camera', 'Profile for this optical setup', 'Best frames kept', 'Disk used', 'Open file manager', 'Store on', 'Use this device’s drive']
    for (const { p, s } of src()) for (const b of banned) expect(s.includes(b), `${p} still says "${b}"`).toBe(false)
  })

  test('the keep-best fields follow one pattern, and the optical setup is called one thing', () => {
    const all = src().map((x) => x.s).join('\n')
    expect(all).toContain('Keep best (%): stacked from a video file')
    expect(all).toContain('Keep best (%): stacked from memory')
    expect(all).not.toMatch(/Best frames kept|Keep best \(%\) /)
    expect(all.match(/Optical setup, in use: /g)!.length).toBeGreaterThanOrEqual(3)
  })

  test('the file manager root and the storage setting have their words', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'More')
    await page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Storage$/ }) }).locator('summary').click()
    await expect(page.getByLabel('Save to')).toBeVisible()
    await expect(page.getByLabel('Used')).toBeVisible()
    await page.getByRole('button', { name: 'Files: recordings and stacks' }).click()
    await expect(page.getByRole('button', { name: 'Captures' }).first()).toBeVisible()
  })

  test('greyed buttons point at their reason (aria-describedby)', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'More')
    const reasoned = async (name: string | RegExp) => {
      const b = page.getByRole('button', { name }).first()
      await expect(b, String(name)).toBeDisabled()
      const id = await b.getAttribute('aria-describedby')
      expect(id, `${name} has aria-describedby`).toBeTruthy()
      await expect(page.locator(`#${id}`), `${name} reason`).not.toBeEmpty()
    }
    const open = (title: string) => page.locator('details.disc', { has: page.locator('.disc-title', { hasText: new RegExp(`^${title}$`) }) }).locator('summary').click()
    await open('Optical setup')
    await reasoned('Use these values')
    await reasoned('Duplicate')
    await reasoned('Delete…')
    await reasoned(/^Apply/)
    await reasoned('Save as new…')
    await open('RAM buffer')
    await reasoned('Set buffer')
    await open('Storage')
    await reasoned('Use this drive')
  })

  test('the status bar idle line is text, not a disabled button', async ({ page }) => {
    await page.goto('/')
    await expect(page.locator('.sbar button:disabled')).toHaveCount(0)
  })

  test('the Auto-Lock note is shown once and remembered', async ({ page }) => {
    await page.goto('/')
    await tab(page, 'More')
    await expect(page.getByText(/set the phone's Auto-Lock to Never/)).toBeVisible()
    await page.getByRole('button', { name: 'Got it' }).click()
    await expect(page.getByText(/set the phone's Auto-Lock to Never/)).toHaveCount(0)
    await expect.poll(async () => (await (await page.request.get('/api/ui')).json())['ui.autolock_seen']).toBe(true)
    await page.reload()
    await tab(page, 'More')
    await expect(page.getByText(/set the phone's Auto-Lock to Never/)).toHaveCount(0)
  })

  test('the offset readout names the pad button once calibrated, and says to calibrate before', async ({ page }) => {
    const sun = (s: any) => { s.sun = { pos: { x: 400, y: 300 }, radius: 200, partial: false } } // eslint-disable-line @typescript-eslint/no-explicit-any
    await patchStatus(page, (s) => { sun(s); s.guiding.calibration = null })
    await page.goto('/')
    await tab(page, 'Aim')
    await expect(page.locator('.offset-press')).toContainText('Calibrate to see which button')
    await page.unrouteAll()
  })

  test('the offset readout with a calibration says which letter to press', async ({ page }) => {
    await patchStatus(page, (s) => { s.sun = { pos: { x: 400, y: 300 }, radius: 200, partial: false }; s.guiding.calibration = { primary_dir: { x: 1, y: 0 }, secondary_dir: { x: 0, y: 1 } } })
    await page.goto('/')
    await tab(page, 'Aim')
    await expect(page.locator('.offset-press')).toContainText(/Press [EW] and [NS]/)
  })

  test('the Track tab preselects the profile\'s own way of following when nothing was chosen', async ({ page }) => {
    for (const [profile, label] of [['planet', 'Planet centroid'], ['other', 'Feature'], ['sun_ha', 'Disc edge'], ['moon', 'Disc edge']] as const) {
      await put(page, '/profile', { id: profile })
      await post(page, '/tracking/stop')
      await page.goto('/')
      await tab(page, 'Track')
      await expect(page.getByRole('radio', { name: label }), `${profile}`).toHaveAttribute('aria-checked', 'true')
    }
    await put(page, '/profile', { id: 'sun_ha' })
  })

  test('the brand colour and the crumb are above 4.5:1 in the day theme', async ({ page }) => {
    await put(page, '/ui', { key: 'ui.theme', value: 'day' })
    await page.goto('/')
    const ratio = await page.evaluate(() => {
      const parse = (c: string) => (c.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number)
      const lum = ([r, g, b]: number[]) => { const f = (v: number) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4 }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b) }
      const cr = (a: number[], b: number[]) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05) }
      const fg = parse(getComputedStyle(document.querySelector('.top h1 span')!).color)
      const bg = parse(getComputedStyle(document.body).backgroundColor)
      return cr(fg, bg)
    })
    expect(ratio).toBeGreaterThanOrEqual(4.5)
    await put(page, '/ui', { key: 'ui.theme', value: 'night' })
  })
})
