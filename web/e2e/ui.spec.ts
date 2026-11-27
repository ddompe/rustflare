import { expect, Page, test } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import path from 'node:path'

const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })

/** Opens the newest day folder in the file manager (the top shows the days, stacks and logs). */
async function openDay(fm: import('@playwright/test').Locator) {
  await fm.locator('.fm-row[data-type="day"]').first().locator('.fm-open').click()
  await expect(fm.locator('.fm-crumbs')).toContainText('›')
}

test.beforeEach(async ({ page }) => {
  await put(page, '/storage/reserve', { gb: 2 }) // the test server records to its own disk: keep its reserve small (a CI runner has little free)
  await post(page, '/camera/close'); await post(page, '/mount/disconnect')
  await post(page, '/camera/open', { driver: 'simulator', id: 'sim0' })
  await post(page, '/mount/connect', { driver: 'simulator' })
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
})

test('every tab renders without horizontal overflow or console errors', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  page.on('console', (m) => { if (m.type() === 'error' && !m.text().startsWith('Failed to load resource')) errors.push(m.text()) })
  page.on('response', (r) => { if (r.status() >= 400) errors.push(`${r.status()} ${r.url()}`) })
  await page.goto('/')
  for (const t of ['Camera', 'Aim', 'Track', 'Record', 'More']) {
    await page.getByRole('tab', { name: new RegExp('^' + t) }).click()
    const over = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)
    expect(over, `${t} overflows horizontally`).toBe(false)
  }
  expect(errors.join('\n')).toBe('')
})

test('the Status overlay carries fps and the flat indicator, once', async ({ page }) => {
  await page.goto('/')
  const overlay = page.getByRole('button', { name: /Status overlay/ })
  await expect(overlay).toBeVisible()
  await expect(overlay).toContainText('fps')
  if (!(await overlay.textContent())?.includes('Flat')) await overlay.click() // expand when minimised (phones)
  await expect(overlay).toContainText('Flat')
  await expect(overlay).toContainText('none')
  // the old duplicate readouts are gone
  await expect(page.locator('.chip.fps')).toHaveCount(0)
})

test('Go to Sun: needs a site, asks for confirmation, ends with solar tracking on', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Aim/ }).click()
  const hasSite = !!(await (await page.request.get('/api/status')).json()).site
  if (!hasSite) await expect(page.getByRole('button', { name: 'Go to Sun' })).toBeDisabled() // no site yet
  // a site where it is local solar noon right now keeps the Sun high at any hour of the test
  const lon = (((12 - (new Date().getUTCHours() + new Date().getUTCMinutes() / 60)) * 15 + 180) % 360) - 180
  expect((await put(page, '/site', { latitude: 0, longitude: lon })).ok()).toBe(true)
  await expect(page.getByRole('button', { name: 'Go to Sun' })).toBeEnabled()
  await page.getByRole('button', { name: 'Go to Sun' }).click()
  const dialog = page.getByRole('alertdialog', { name: 'Go to the Sun' })
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: 'Go to Sun' }).click()
  await expect(page.getByText(/Going to the Sun/)).toBeVisible()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).mount.tracking, { timeout: 15_000 }).toBe(true)
})

test('Go to the Moon and planets: one target selector, a greyed-out button below the horizon, tracking at the right rate', async ({ page }) => {
  await put(page, '/site', { latitude: 40, longitude: -105 })
  await page.goto('/')
  await page.getByRole('tab', { name: /^Aim/ }).click()
  const status = async () => (await page.request.get('/api/status')).json()
  await expect.poll(async () => (await status()).sky_targets?.length ?? 0).toBe(4)
  const targets: { id: string; name: string; altitude_deg: number }[] = (await status()).sky_targets
  expect(targets.map((t) => t.id)).toEqual(['moon', 'mars', 'jupiter', 'saturn'])
  const select = page.getByLabel('Go to target')
  await expect(select.locator('option')).toHaveCount(5)
  for (const t of targets) {
    await select.selectOption(t.id)
    const goBtn = page.getByRole('button', { name: `Go to ${t.name}` })
    // below the horizon: the button is greyed out and the page says why
    if (t.altitude_deg < -1) {
      await expect(goBtn).toBeDisabled()
      await expect(page.getByText(/below the horizon: Go to is off/)).toBeVisible()
      continue
    }
    await expect(goBtn).toBeEnabled()
    await goBtn.click()
    const dialog = page.getByRole('alertdialog', { name: `Go to ${t.name}` })
    await expect(dialog).toBeVisible()
    await expect(dialog.getByRole('button', { name: `Go to ${t.name}` })).toBeVisible()
    await dialog.getByRole('button', { name: 'Cancel' }).click()
  }
  // the API: unknown targets are refused, below the horizon needs the explicit flag, and the mount tracks at the target's rate
  expect((await post(page, '/mount/goto', { target: 'pluto', confirm: true })).status()).toBe(400)
  expect((await post(page, '/mount/goto', { target: 'moon' })).status()).toBe(400) // no confirmation
  const down = targets.find((t) => t.altitude_deg < -1)
  if (down) {
    const r = await post(page, '/mount/goto', { target: down.id, confirm: true })
    expect(await r.text()).toContain('below the horizon')
  }
  const up = targets.find((t) => t.altitude_deg > 5) ?? down!
  const r = await post(page, '/mount/goto', { target: up.id, confirm: true, allow_below_horizon: true })
  expect(r.ok()).toBe(true)
  await expect.poll(async () => (await status()).mount.tracking, { timeout: 20_000 }).toBe(true)
  expect((await status()).mount.rate).toBe(up.id === 'moon' ? 'lunar' : 'sidereal')
  await select.selectOption('sun')
})

test('record a short sequence, check it, delete it', async ({ page }) => {
  await page.goto('/')
  // the first frame can still be on its way after the camera was (re)opened
  await expect.poll(async () => (await post(page, '/capture/sequence', { name: 'e2e', frames: 10, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 15_000 }).toBe('idle')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await expect(fm).toBeVisible()
  await openDay(fm)
  const row = fm.locator('.fm-row', { hasText: 'e2e' })
  await expect(row).toHaveCount(1) // the .json notes file is not a row of its own
  await row.getByRole('button', { name: 'Check' }).click()
  await expect(row).toContainText('10/10 frames')
  await expect(row).not.toContainText('Mostly dark')
  await row.getByRole('checkbox').check()
  await fm.getByRole('button', { name: /Delete 1 item/ }).click()
  await fm.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
  await expect(fm.locator('.fm-row', { hasText: 'e2e' })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(fm).toHaveCount(0)
})

test('flats and darks live in the Record tab, with the other everyday work', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await expect(page.getByText('Flats', { exact: true })).toBeVisible()
  await page.getByText('Flats', { exact: true }).click()
  await expect(page.getByRole('button', { name: /Start flats/ })).toBeVisible()
  await expect(page.getByText('Darks', { exact: true })).toBeVisible()
  for (const t of [/^Camera/, /^More/]) {
    await page.getByRole('tab', { name: t }).click()
    await expect(page.getByText('Flats', { exact: true })).toHaveCount(0)
  }
})

test('Prominence view toggles from the live view tools', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => !!(await (await page.request.get('/api/status')).json()).sun, { timeout: 10_000 }).toBe(true).catch(() => {})
  const btn = page.getByRole('button', { name: 'Prominence view' })
  if (await btn.isDisabled()) test.skip(true, 'the simulator scene has no Sun in view')
  await btn.click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).coronagraph.enabled).toBe(true)
  await expect(btn).toHaveAttribute('aria-pressed', 'true') // the UI follows the server
  await btn.click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).coronagraph.enabled).toBe(false)
})

test('frame size (ROI) changes the picture size and restores', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Camera/ }).click()
  await page.getByText('Frame size').first().click()
  await page.getByRole('radio', { name: '50 %' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).camera.width, { timeout: 10_000 }).toBeLessThan(1280)
  await page.getByRole('radio', { name: 'Full' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).camera.width, { timeout: 10_000 }).toBe(1280)
})

test('Record offers sequence, prominence and HDR lanes in one form', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('radio', { name: 'HDR lanes' }).check()
  await page.getByRole('button', { name: 'Diamond ring' }).click()
  await expect(page.getByLabel('Lane 3 name')).toHaveValue('e3')
  await page.getByRole('radio', { name: '+ Prominence' }).check()
  await expect(page.getByText('Save the disc position with every prominence frame')).toBeVisible()
  await page.getByRole('radio', { name: 'Sequence' }).check()
  await expect(page.locator('.quick-rec')).toBeVisible()
})

test('HDR lanes record into one folder per lane', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => (await post(page, '/capture/lanes', { lanes: [{ name: 'x1', exposure_us: 200, frames: 3 }, { name: 'x2', exposure_us: 400, frames: 3 }], cycles: 1 })).text(), { timeout: 10_000 }).toContain('lanes')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 20_000 }).toBe('idle')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await openDay(fm)
  await expect(fm).toContainText('hdr_')
  await expect(fm.locator('.fm-row[data-type="hdr"]').first()).toContainText('HDR lanes')
})

test('the prominence recording with the coronagraph masks the disc in its lane only', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('radio', { name: '+ Prominence' }).check()
  await page.getByLabel('Frames per lane').fill('3')
  await page.getByRole('switch', { name: /Save the disc position/ }).check()
  await page.getByLabel('Number of cycles').fill('1')
  await page.locator('.quick-rec').click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 20_000 }).toBe('idle')
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await openDay(fm)
  await expect(fm.locator('.fm-row[data-type="prominence"]').first()).toContainText('Prominence video')
  await fm.locator('.fm-row[data-type="prominence"]').first().locator('.fm-open').click()
  await expect(fm.locator('.fm-row[data-type="lane"]')).toHaveCount(2)
})

test('focus assist scores the image, falls when defocused and keeps a best marker', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Camera/ }).click()
  await page.getByText('Focus assist').first().click()
  await page.getByRole('switch', { name: 'Focus assist' }).check()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).focus.history.length, { timeout: 15_000 }).toBeGreaterThan(1)
  const sharp = (await (await page.request.get('/api/status')).json()).focus.lucky
  await page.request.put('/api/camera/control', { data: { id: 'sim_defocus', value: 12 } })
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).focus.lucky, { timeout: 15_000 }).toBeLessThan(sharp * 0.8)
  await expect(page.locator('.focus-score').getByText(/best \d/)).toBeVisible()
  await expect(page.getByRole('img', { name: /Focus score history/ })).toBeVisible()
  await page.request.put('/api/camera/control', { data: { id: 'sim_defocus', value: 0 } })
  await page.getByRole('switch', { name: 'Focus assist' }).uncheck()
})

test('quick Rec and Snap are on the Camera tab and use the saved recipe', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('radio', { name: 'Sequence' }).count()
  const quick = page.getByRole('group', { name: 'Quick capture' })
  await expect(quick).toBeVisible()
  await quick.getByRole('button', { name: 'Snap' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.last_file, { timeout: 10_000 }).toBeTruthy()
  await page.request.post('/api/capture/stop')
})

/** A longitude in [-180, 180): the meridian arithmetic below runs past the date line at some times of day. */
const wrapLon = (l: number) => ((((l + 180) % 360) + 360) % 360) - 180

test('automatic meridian flip: needs a site, switches on with its timing, and off again', async ({ page }) => {
  // a site where the Sun crosses the meridian in about three hours, whatever the time of day: the automatic flip acts from 35 minutes before
  // the meridian to 90 minutes after it, and this test only switches it on and off again
  await put(page, '/site', { latitude: 40, longitude: 0 })
  const m0 = ((await (await page.request.get('/api/status')).json()) as { sky_sun: { minutes_to_meridian: number } }).sky_sun.minutes_to_meridian
  await put(page, '/site', { latitude: 40, longitude: wrapLon((m0 - 180) / 4) })
  await page.goto('/')
  await page.getByRole('tab', { name: /^Aim/ }).click()
  await page.getByText(/Meridian flip/i).first().click()
  const sw = page.getByRole('switch', { name: 'Flip automatically' })
  await expect(sw).toBeVisible()
  await sw.click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).auto_flip?.enabled).toBe(true)
  await expect(page.getByLabel('Record before the meridian')).toBeVisible()
  await sw.click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).auto_flip?.enabled).toBe(false)
})

test('a message in the status bar does not move the controls', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => !!(await (await page.request.get('/api/status')).json()).sun, { timeout: 10_000 }).toBe(true).catch(() => {})
  await post(page, '/mount/tracking', { enabled: true })
  await post(page, '/tracking/start', { mode: 'limb' })
  await page.waitForTimeout(1000)
  const box = async () => JSON.stringify(await page.locator('.panel').boundingBox()) + JSON.stringify(await page.locator('.live').boundingBox())
  const bar = page.locator('.sbar')
  await expect(bar).toContainText('Ready')
  const before = await box()
  const h0 = (await bar.boundingBox())!.height
  await put(page, '/camera/control', { id: 'sim_zoom', value: 8 }) // the limb leaves the frame: the target is lost
  await expect(bar).toContainText(/lost/i, { timeout: 10_000 })
  expect(await box()).toBe(before)
  expect((await bar.boundingBox())!.height).toBe(h0)
  await put(page, '/camera/control', { id: 'sim_zoom', value: 0.9 })
  await post(page, '/tracking/stop')
})

test('mask size and focus score are overlay widgets that start apart', async ({ page }) => {
  await expect.poll(async () => !!(await (await page.request.get('/api/status')).json()).sun, { timeout: 10_000 }).toBe(true).catch(() => {})
  const hasSun = !!(await (await page.request.get('/api/status')).json()).sun
  test.skip(!hasSun, 'the simulator scene has no Sun in view')
  await expect.poll(async () => (await put(page, '/coronagraph', { enabled: true, scale: 1.03 })).ok(), { timeout: 10_000 }).toBe(true)
  await expect.poll(async () => (await put(page, '/focus', { enabled: true })).ok(), { timeout: 10_000 }).toBe(true)
  await page.goto('/')
  const mask = page.getByRole('button', { name: /Prominence mask size overlay/ })
  await expect(mask).toBeVisible()
  await expect(page.getByRole('button', { name: /Focus score overlay/ })).toBeVisible()
  const boxes = await page.locator('.hud-w').evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom, e.getAttribute('aria-label') ?? ''] as [number, number, number, number, string] }))
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
    const [a, b] = [boxes[i], boxes[j]]
    expect(a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3], `widgets overlap: ${JSON.stringify(a)} and ${JSON.stringify(b)}`).toBe(false)
  }
  // the slider inside changes the mask without moving the widget
  const before = await mask.boundingBox()
  await mask.getByRole('slider').fill('1.1')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).coronagraph.scale).toBeCloseTo(1.1, 2)
  expect(await mask.boundingBox()).toEqual(before)
  await put(page, '/coronagraph', { enabled: false, scale: 1.03 })
  await put(page, '/focus', { enabled: false })
})

test('a recording can be paused and resumed, and discarded', async ({ page }) => {
  await page.goto('/')
  const status = async () => (await (await page.request.get('/api/status')).json()).capture
  await expect.poll(async () => (await post(page, '/capture/lanes', { name: 'pz', cycles: 50, cycle_interval_s: 2, format: 'ser', lanes: [{ name: 'a', exposure_us: 1000, frames: 3 }, { name: 'b', exposure_us: 3000, frames: 3 }] })).ok(), { timeout: 10_000 }).toBe(true)
  await expect.poll(async () => (await status()).state).toBe('bursts')
  await page.getByRole('button', { name: 'Pause' }).click()
  await expect.poll(async () => { const c = await status(); return c.state === 'idle' && c.paused }, { timeout: 15_000 }).toBe(true)
  await expect(page.getByText('Recording paused')).toBeVisible()
  await page.getByRole('button', { name: 'Resume' }).click()
  await expect.poll(async () => (await status()).state, { timeout: 15_000 }).toBe('bursts')
  expect((await status()).paused).toBe(false)
  await page.getByRole('button', { name: 'Pause' }).click()
  await expect.poll(async () => (await status()).paused, { timeout: 15_000 }).toBe(true)
  await page.getByRole('button', { name: 'Discard' }).click()
  await expect.poll(async () => (await status()).paused, { timeout: 10_000 }).toBe(false)
})

test('the Record tab offers Pause recording, Resume recording and Discard', async ({ page }) => {
  await page.goto('/')
  const status = async () => (await (await page.request.get('/api/status')).json()).capture
  await expect.poll(async () => (await post(page, '/capture/lanes', { name: 'pz2', cycles: 50, cycle_interval_s: 2, format: 'ser', lanes: [{ name: 'a', exposure_us: 1000, frames: 3 }, { name: 'b', exposure_us: 3000, frames: 3 }] })).ok(), { timeout: 10_000 }).toBe(true)
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: 'Pause', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Resume recording' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Resume recording' }).click()
  await expect.poll(async () => (await status()).state, { timeout: 15_000 }).toBe('bursts')
  await page.getByRole('button', { name: 'Stop recording' }).click()
  await expect.poll(async () => (await status()).state, { timeout: 15_000 }).toBe('idle')
})

test('the Record tab estimates when a repeating recording ends', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('radio', { name: 'Sequence' }).check()
  await page.getByLabel('Frames', { exact: true }).fill('20')
  await page.getByLabel(/Repeat every/).fill('30')
  await page.getByLabel(/Number of repeats/).fill('') // an earlier test may have left a number in the saved settings
  await expect(page.getByText(/until you stop it/)).toBeVisible() // no number of repeats: runs until stopped
  await page.getByLabel(/Number of repeats/).fill('4')
  await expect(page.getByText(/Ends about \d\d?:\d\d/)).toBeVisible()
  await page.getByLabel(/Number of repeats/).fill('')
  await page.getByLabel(/Repeat every/).fill('')
})

test('the mount goes home when a recording finishes, only if asked', async ({ page }) => {
  await page.goto('/')
  await post(page, '/mount/tracking', { enabled: true })
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('radio', { name: 'Sequence' }).check()
  await page.getByLabel('Frames', { exact: true }).fill('3')
  await page.getByLabel(/Repeat every/).fill('')
  await page.getByText('Advanced', { exact: true }).click()
  const sw = page.getByRole('switch', { name: 'Send the mount home when the recording finishes' })
  await expect(sw).toBeEnabled()
  const mount = async () => (await (await page.request.get('/api/status')).json()).mount
  // off: nothing moves
  await page.locator('.quick-rec').click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 15_000 }).toBe('idle')
  await page.waitForTimeout(1500)
  expect((await mount()).at_home).toBe(false)
  // on: the mount is sent home after the recording ends by itself (the Record tab was rebuilt while recording: open Advanced again)
  if (!(await sw.isVisible())) await page.getByText('Advanced', { exact: true }).click()
  await sw.check()
  await page.locator('.quick-rec').click()
  await expect.poll(async () => (await mount()).at_home, { timeout: 20_000 }).toBe(true)
  if (!(await sw.isVisible())) await page.getByText('Advanced', { exact: true }).click()
  await sw.uncheck()
  await post(page, '/mount/tracking', { enabled: true })
})

test('Stacking: the stacker, its settings, and the stacks it made are looked at in Files', async ({ page }) => {
  // record a short burst on the simulator and stack it the way the stacker service would (the recorder's folder, a stacks/ subfolder)
  await expect.poll(async () => (await post(page, '/capture/sequence', { name: 'stackme', frames: 12, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 20_000 }).toBe('idle')
  await page.waitForTimeout(1500)
  execFileSync(path.resolve('../target/release/rustflare-stack'), ['session', path.resolve('.e2e-data/data'), '--threads', '2'])

  await page.goto('/')
  await page.getByRole('tab', { name: /^More/ }).click()
  const section = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: 'Stacking' }) })
  await section.locator('summary').click()
  // not running here (no service): says it restarts by itself, no shell command
  await expect(section.getByText(/starts it again by itself/)).toBeVisible()
  // the panel does not list the stacks: it counts them and opens Files in the stacks folder, where they are shown and downloaded
  await expect(section.locator('.stack-card')).toHaveCount(0)
  await expect(section.getByText(/\d+ stacks so far/)).toBeVisible({ timeout: 10_000 })
  await section.getByRole('button', { name: 'Open the stacks in Files' }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await expect(fm.locator('.fm-crumbs')).toContainText('stacks')
  await fm.locator('.fm-row[data-type="folder"] .fm-open, .fm-row[data-type="day"] .fm-open').first().click()
  const cell = fm.getByRole('list', { name: 'Stacks' }).locator('li').first()
  await expect(cell).toBeVisible({ timeout: 10_000 })
  await expect.poll(async () => cell.locator('img').evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true)
  await page.keyboard.press('Escape')
  // settings are saved by the server and survive a reload
  await section.getByText('Stacker settings', { exact: true }).click()
  await section.getByRole('radio', { name: 'Always' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/stack')).json()).config.policy).toBe('always')
  await section.getByRole('radio', { name: 'When idle' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/stack')).json()).config.policy).toBe('when_idle')
  expect((await put(page, '/stack/config', { policy: 'sometimes' })).status()).toBe(400)
  expect((await put(page, '/stack/config', { flat: '../etc/passwd' })).status()).toBe(400)
  const kept = await put(page, '/stack/config', { keep_percent: 500, threads: 64, policy: 'between_bursts' })
  expect((await kept.json()).keep_percent).toBe(100) // clamped
  await put(page, '/stack/config', { keep_percent: 25, threads: 2 })
})

test('tracking rate: a Go to leaves the rate of its target on, and switching tracking off and on keeps it; the rate can be chosen', async ({ page }) => {
  await put(page, '/site', { latitude: 40, longitude: -105 })
  const status = async () => (await page.request.get('/api/status')).json()
  expect((await post(page, '/mount/goto', { target: 'saturn', confirm: true, allow_below_horizon: true })).ok()).toBe(true)
  await expect.poll(async () => (await status()).mount.tracking, { timeout: 20_000 }).toBe(true)
  expect((await status()).mount.rate).toBe('sidereal')
  await page.goto('/')
  await page.getByRole('tab', { name: /^Aim/ }).click()
  await expect(page.getByText('Star tracking')).toBeVisible()
  await page.getByRole('group', { name: 'Tracking rate' }).getByRole('button', { name: 'Lunar' }).click()
  await expect.poll(async () => (await status()).mount.rate).toBe('lunar')
  await post(page, '/mount/tracking', { enabled: false })
  await post(page, '/mount/tracking', { enabled: true }) // no rate given: the last one stays
  expect((await status()).mount.rate).toBe('lunar')
  await page.getByRole('group', { name: 'Tracking rate' }).getByRole('button', { name: 'Solar' }).click()
  await expect.poll(async () => (await status()).mount.rate).toBe('solar')
})

test('After each burst: modes, a stacker that is not there is never quiet, notices stay until dismissed', async ({ page }) => {
  const status = async () => (await page.request.get('/api/status')).json()
  await put(page, '/capture/after-burst', { mode: 'video' })
  // the API: stack-only needs the stacker, values are checked
  const r = await put(page, '/capture/after-burst', { mode: 'stack_only' })
  expect(r.status()).toBe(400)
  expect(await r.text()).toContain('stacker is not running')
  expect((await put(page, '/capture/after-burst', { margin: 9 })).status()).toBe(400)
  expect((await put(page, '/capture/after-burst', { raw_keep_percent: 0 })).status()).toBe(400)
  expect((await put(page, '/capture/after-burst', { mode: 'nonsense' })).status()).toBe(400)
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByText('After each burst', { exact: true }).click()
  await expect(page.getByRole('button', { name: /Keep the video/ })).toHaveAttribute('aria-pressed', 'true')
  await page.getByRole('button', { name: /Stack only/ }).click()
  await expect(page.getByText(/stacker is not running/).first()).toBeVisible()
  await expect(page.getByRole('button', { name: /Keep the video/ })).toHaveAttribute('aria-pressed', 'true') // refused: nothing changed
  // video + stack without a stacker: allowed, but every recording says it is video only, in the status bar, until dismissed
  await page.getByRole('button', { name: /Video and stack/ }).click()
  await expect(page.getByRole('button', { name: /Video and stack/ })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByText(/Stacker ready|The stacker is not running, so bursts are saved as video for now/)).toBeVisible()
  expect((await post(page, '/capture/sequence', { name: 'noticetest', frames: 8, format: 'ser' })).ok()).toBe(true)
  const bar = page.locator('.sbar')
  await expect(bar).toContainText('recorded as video', { timeout: 15_000 })
  await expect(bar.getByRole('button', { name: 'Dismiss' })).toBeVisible()
  await expect.poll(async () => (await status()).capture.state, { timeout: 20_000 }).toBe('idle')
  await expect(bar).toContainText('recorded as video') // still there after the recording
  await bar.getByRole('button', { name: 'Dismiss' }).click()
  await expect(bar).not.toContainText('recorded as video')
  expect(((await status()).notices as { acked: boolean }[]).every((n) => n.acked)).toBe(true)
  await put(page, '/capture/after-burst', { mode: 'video' })
  const files = (await (await page.request.get('/api/files')).json()) as { files?: { path: string }[] }
  for (const f of files.files ?? []) if (f.path.includes('noticetest')) await page.request.delete(`/api/files/${f.path}`)
})

test('the file manager browses folders: stacks and a folder download are reachable', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => (await post(page, '/capture/sequence', { name: 'fmnav', frames: 5, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 15_000 }).toBe('idle')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await openDay(fm)
  await expect(fm.locator('.fm-row', { hasText: 'fmnav' })).toHaveCount(1)
  // the breadcrumb goes back to the top, and the filter narrows a folder
  await fm.locator('.fm-crumbs').getByRole('button', { name: 'Drive', exact: true }).click()
  await expect(fm.locator('.fm-row[data-type="day"]').first()).toBeVisible()
  await openDay(fm)
  await fm.getByLabel('Filter this folder by name').fill('no-such-name')
  await expect(fm).toContainText('Nothing in this folder matches')
  await fm.getByLabel('Filter this folder by name').fill('')
  // the whole day as one tar: the server answers with a tar of the right size
  const href = await fm.getByRole('link', { name: /Download this folder/ }).getAttribute('href')
  const r = await page.request.get(href!)
  expect(r.status()).toBe(200)
  expect(r.headers()['content-type']).toBe('application/x-tar')
  const body = await r.body()
  expect(body.length).toBe(Number(r.headers()['content-length']))
  expect(body.length % 512).toBe(0)
  // the stacker's record of what it has done cannot be deleted
  const del = await post(page, '/files/delete', { path: 'stacks/manifest.json', confirm: true })
  expect(del.status()).toBe(400)
  // clean up
  await fm.locator('.fm-row', { hasText: 'fmnav' }).getByRole('checkbox').check()
  await fm.getByRole('button', { name: /Delete 1 item/ }).click()
  await fm.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
  await expect(fm.locator('.fm-row', { hasText: 'fmnav' })).toHaveCount(0)
})

test('the folder tree divider can be dragged and moved with the keyboard', async ({ page }, info) => {
  test.skip(info.project.name === 'phone', 'the tree only shows on wide screens')
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  const tree = fm.getByRole('navigation', { name: 'Folders' })
  const w0 = (await tree.boundingBox())!.width
  const bar = fm.getByRole('separator', { name: 'Resize the folder tree' })
  const b = (await bar.boundingBox())!
  await page.mouse.move(b.x + b.width / 2, b.y + 200)
  await page.mouse.down(); await page.mouse.move(b.x + 120, b.y + 200, { steps: 5 }); await page.mouse.up()
  await expect.poll(async () => (await tree.boundingBox())!.width).toBeGreaterThan(w0 + 80)
  const w1 = (await tree.boundingBox())!.width
  await bar.focus(); await page.keyboard.press('ArrowLeft')
  await expect.poll(async () => (await tree.boundingBox())!.width).toBeLessThan(w1)
  await bar.dblclick()
  await expect.poll(async () => Math.round((await tree.boundingBox())!.width)).toBe(240)
})

test('a failed action is shown in the status bar until dismissed, never swallowed', async ({ page }) => {
  await page.route('**/api/storage', (r) => r.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: 'the drive is on fire' }) }))
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click() // reads the storage state
  const bar = page.locator('.sbar')
  await expect(bar).toContainText('Reading the storage state failed: the drive is on fire')
  await page.keyboard.press('Escape')
  await bar.getByRole('button', { name: 'Dismiss' }).click()
  await expect(bar).not.toContainText('the drive is on fire')
})

test('stacks show as thumbnails with a preview, and a selection downloads as one tar', async ({ page }) => {
  const fs = await import('node:fs')
  const dir = path.join(process.cwd(), '.e2e-data/data/stacks/2099-01-01')
  fs.mkdirSync(dir, { recursive: true })
  const fits = (w: number, h: number) => {
    const card = (s: string) => s.padEnd(80)
    const head = Buffer.from([card('SIMPLE  =                    T'), card('BITPIX  =                  -32'), card('NAXIS   =                    2'), card(`NAXIS1  = ${String(w).padStart(20)}`), card(`NAXIS2  = ${String(h).padStart(20)}`), card('END')].join('').padEnd(2880))
    const body = Buffer.alloc(Math.ceil((w * h * 4) / 2880) * 2880)
    for (let i = 0; i < w * h; i++) body.writeFloatBE(((i % w) / w) * 40000 + 100, i * 4)
    return Buffer.concat([head, body])
  }
  for (const n of ['stack_a_001.fits', 'stack_a_002.fits', 'stack_a_003.fits']) fs.writeFileSync(path.join(dir, n), fits(64, 48))
  try {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Record/ }).click()
    await page.getByRole('button', { name: /Open file manager/ }).click()
    const fm = page.getByRole('dialog', { name: 'File manager' })
    await fm.locator('.fm-row[data-type="stacks"] .fm-open').click()
    await fm.locator('.fm-row', { hasText: '2099-01-01' }).locator('.fm-open').click()
    const cells = fm.getByRole('list', { name: 'Stacks' }).locator('li')
    await expect(cells).toHaveCount(3)
    await cells.first().getByRole('button', { name: /Preview/ }).click()
    const peek = page.getByRole('dialog', { name: /^Preview of stack_a_001/ })
    await expect(peek).toContainText('1 of 3')
    await peek.getByRole('button', { name: /Next/ }).click()
    await expect(page.getByRole('dialog', { name: /^Preview of stack_a_002/ })).toContainText('2 of 3')
    await page.keyboard.press('Escape')
    await expect(page.locator('.fm-peek')).toHaveCount(0)
    await expect(fm).toBeVisible() // Escape closed only the preview
    await cells.nth(0).getByRole('checkbox').check()
    await cells.nth(2).getByRole('checkbox').check()
    const href = await fm.getByRole('link', { name: /Download 2 selected items/ }).getAttribute('href')
    const r = await page.request.get(href!)
    expect(r.status()).toBe(200)
    const body = await r.body()
    expect(body.length).toBe(Number(r.headers()['content-length']))
    const names = body.toString('latin1').match(/stack_a_00\d\.fits/g) ?? []
    expect(new Set(names)).toEqual(new Set(['stack_a_001.fits', 'stack_a_003.fits']))
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('direct writes can be switched on, are used for a recording, and the file is a valid SER', async ({ page }) => {
  const sw = await put(page, '/capture/direct-io', { enabled: true })
  test.skip(sw.status() === 400, 'this filesystem does not allow O_DIRECT')
  expect(sw.status()).toBe(200)
  try {
    await expect.poll(async () => (await post(page, '/capture/sequence', { name: 'directio', frames: 12, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
    await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 15_000 }).toBe('idle')
    const st = (await (await page.request.get('/api/status')).json()).capture.direct_io
    expect(st.wanted && st.active).toBe(true)
    const files = (await (await page.request.get('/api/files/list?path=')).json()).entries as { name: string }[]
    const day = files.filter((e) => /^\d{4}-/.test(e.name)).map((e) => e.name).sort().pop()!
    const list = (await (await page.request.get(`/api/files/list?path=${day}`)).json()).entries as { name: string; path: string }[]
    const ser = list.find((e) => e.name.includes('directio') && e.name.endsWith('.ser'))!
    const stats = await (await page.request.get(`/api/files/stats?path=${encodeURIComponent(ser.path)}`)).json()
    expect(stats.frames_present).toBe(12)
    expect(stats.complete).toBe(true)
    await post(page, '/files/delete', { path: ser.path, confirm: true })
  } finally {
    await put(page, '/capture/direct-io', { enabled: false })
  }
})

test('auto-stretch is a button on the live view and stays in step with More > Display', async ({ page }) => {
  const stretch = async () => ((await (await page.request.get('/api/preview')).json()) as { stretch: boolean }).stretch
  await page.request.put('/api/preview', { data: { stretch: false } })
  await page.goto('/')
  const btn = page.getByRole('button', { name: 'Auto-stretch' })
  await expect(btn).toHaveAttribute('aria-pressed', 'false')
  await btn.click()
  await expect(btn).toHaveAttribute('aria-pressed', 'true')
  await expect.poll(stretch).toBe(true)
  await page.getByRole('tab', { name: /^More/ }).click()
  await page.getByText('Display', { exact: true }).first().click() // the section that holds the switch
  await expect(page.getByRole('switch', { name: 'Auto-stretch' })).toBeChecked()
  await page.getByRole('switch', { name: 'Auto-stretch' }).uncheck()
  await expect(btn).toHaveAttribute('aria-pressed', 'false')
  await expect.poll(stretch).toBe(false)
})

test('the Pi\'s own disk can be recorded to, and always keeps a reserve free', async ({ page }) => {
  const get = async () => (await (await page.request.get('/api/storage')).json()) as { current: { internal?: boolean; reserve_bytes?: number; label: string }; candidates: { internal?: boolean }[] }
  const s = await get()
  expect(s.current.internal).toBe(true) // the test server records to its own data folder
  expect(s.candidates.some((c) => c.internal)).toBe(true)
  expect((await put(page, '/storage/reserve', { gb: 1 })).status()).toBe(400)
  expect((await put(page, '/storage/reserve', { gb: 5 })).status()).toBe(200)
  expect((await get()).current.reserve_bytes).toBe(5_000_000_000)
  await page.goto('/')
  await page.getByRole('tab', { name: /^More/ }).click()
  await page.getByText('Storage', { exact: true }).first().click()
  await expect(page.getByRole('note')).toContainText("Pi's SD card")
  await expect(page.getByLabel('Keep this much free (GB)')).toHaveValue('5')
  await put(page, '/storage/reserve', { gb: 2 })
})

test('the automatic flip switched on after the meridian does nothing: a mount already flipped is never turned back', async ({ page }) => {
  // a site where the Sun crossed the meridian an hour ago, whatever the time of day
  await put(page, '/site', { latitude: 40, longitude: 0 })
  const m0 = ((await (await page.request.get('/api/status')).json()) as { sky_sun: { minutes_to_meridian: number } }).sky_sun.minutes_to_meridian
  await put(page, '/site', { latitude: 40, longitude: wrapLon((m0 + 60) / 4) })
  const status = async () => (await (await page.request.get('/api/status')).json()) as { auto_flip: { stage: string; message: string }; flip: unknown; last_flip: unknown }
  const before = (await status()).last_flip
  await put(page, '/mount/autoflip', { enabled: true, record_min: 5, flip_min: 1 })
  await expect.poll(async () => (await status()).auto_flip.stage, { timeout: 10_000 }).toBe('late')
  expect((await status()).auto_flip.message).toContain('no flip this time')
  await page.waitForTimeout(3000) // it has had time to flip, if it were going to
  const s = await status()
  expect(s.flip).toBeFalsy()
  expect(JSON.stringify(s.last_flip)).toBe(JSON.stringify(before))
  await put(page, '/mount/autoflip', { enabled: false, record_min: 5, flip_min: 1 })
})

test('a drive on a USB 3 port next to a USB 3 camera shows a warning where the drive is chosen', async ({ page }) => {
  const warning = 'This drive is on a USB 3 port, like Apollo-M MINI. Move the drive to a black USB 2.0 port.'
  await page.route('**/api/storage', async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    const r = await route.fetch()
    const j = await r.json()
    j.current.warning = warning
    await route.fulfill({ response: r, json: j })
  })
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'USB 2.0 port' }).first()).toBeVisible()
  await page.getByRole('tab', { name: /^More/ }).click()
  await page.getByText('Storage', { exact: true }).first().click()
  await expect(page.getByRole('alert').filter({ hasText: 'USB 2.0 port' }).first()).toBeVisible()
})


test('the stacker settings are on the Record tab too, and change the same settings', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByText('After each burst', { exact: true }).first().click()
  await page.getByText('Stacker settings', { exact: false }).first().click()
  const cfg = async () => ((await (await page.request.get('/api/stack')).json()) as { config: { multipoint: boolean; disc_mask: boolean; disc_scale: number } }).config
  const before = await cfg()
  const sw = page.getByRole('switch', { name: 'Multi-point alignment' })
  await expect(sw).toBeChecked({ checked: before.multipoint })
  await sw.click()
  await expect.poll(async () => (await cfg()).multipoint).toBe(!before.multipoint)
  await sw.click()
  await expect.poll(async () => (await cfg()).multipoint).toBe(before.multipoint)
  await expect(page.getByRole('switch', { name: 'Black disc on prominence stacks' })).toBeVisible()
})

test('the software version is in the corner of the status bar', async ({ page }) => {
  await page.goto('/')
  const v = ((await (await page.request.get('/api/status')).json()) as { version: string }).version
  expect(v).toMatch(/^\d+\.\d+\.\d+/)
  await expect(page.locator('.sbar .sbar-ver')).toHaveText(`v${v}`)
})

test.describe('cooled camera', () => {
  const status = async (page: Page) => (await (await page.request.get('/api/status')).json()) as { camera?: { temperature_c: number | null; cooler: { available: boolean; on: boolean; target_c: number | null; power_pct: number | null; warming_up: boolean; fan: boolean | null; heater: boolean | null } } }
  const openCooled = async (page: Page) => {
    await post(page, '/camera/close')
    await post(page, '/camera/open', { driver: 'simulator', id: 'sim_cooled' })
    await expect.poll(async () => (await status(page)).camera?.cooler.available).toBe(true)
  }
  test.afterEach(async ({ page }) => { await put(page, '/camera/cooler', { on: false, immediate: true }) })

  test('an uncooled camera has no cooler section and the API refuses', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await expect(page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Cooler$/ }) })).toHaveCount(0)
    expect((await status(page)).camera?.cooler.available).toBe(false)
    expect((await put(page, '/camera/cooler', { on: true })).ok()).toBe(false)
  })

  test('the cooler section switches the cooler, sets the target and shows power and temperature', async ({ page }) => {
    await openCooled(page)
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    // the cooler settings are a collapsible section: closed, its summary says the state; open, the controls show
    const cooler = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Cooler$/ }) })
    await expect(cooler).toBeVisible()
    await expect(page.getByRole('switch', { name: 'Fan' })).toBeHidden()
    await cooler.locator('summary').click()
    await expect(page.getByRole('switch', { name: 'Fan' })).toBeVisible()
    await expect(page.getByRole('switch', { name: 'Anti-dew heater' })).toBeVisible()
    const sw = page.getByRole('switch', { name: /^Cooler/ })
    await expect(sw).not.toBeChecked()
    expect((await put(page, '/camera/cooler', { target_c: -20 })).ok()).toBe(true)
    await expect.poll(async () => (await status(page)).camera?.cooler.target_c).toBe(-20)
    await sw.click()
    await expect.poll(async () => (await status(page)).camera?.cooler.on).toBe(true)
    // the simulated sensor cools about a degree a second and the cooler works for it
    await expect.poll(async () => (await status(page)).camera?.temperature_c ?? 99, { timeout: 15_000 }).toBeLessThan(20)
    await expect(page.getByTestId('cooler-power')).not.toHaveText('0 %', { timeout: 15_000 })
    await page.getByRole('switch', { name: 'Anti-dew heater' }).click()
    await expect.poll(async () => (await status(page)).camera?.cooler.heater).toBe(true)
  })

  test('switching off a cold sensor warms it gradually, and can be forced', async ({ page }) => {
    await openCooled(page)
    await put(page, '/camera/cooler', { target_c: -20, on: true })
    await expect.poll(async () => (await status(page)).camera?.temperature_c ?? 99, { timeout: 15_000 }).toBeLessThan(10)
    const r = await put(page, '/camera/cooler', { on: false })
    expect(((await r.json()) as { warming_up: boolean }).warming_up).toBe(true)
    const c = (await status(page)).camera!.cooler
    expect(c.on).toBe(true) // still on: the target is raised step by step
    expect(c.warming_up).toBe(true)
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Cooler$/ }) }).locator('summary').click()
    await expect(page.getByText('Warming the sensor before switching off')).toBeVisible()
    await page.getByRole('button', { name: 'Switch off now' }).click()
    await expect.poll(async () => (await status(page)).camera?.cooler.on).toBe(false)
    expect((await status(page)).camera?.cooler.warming_up).toBe(false)
  })
})

test.describe('use profiles: the words and the enabled tools follow what is imaged, nothing is hidden', () => {
  const setProfile = (page: Page, id: string) => put(page, '/profile', { id })
  const profile = async (page: Page) => ((await (await page.request.get('/api/status')).json()) as { profile: string }).profile
  test.afterEach(async ({ page }) => { await setProfile(page, 'sun_ha') })

  test('the profile is set through the API, validated, remembered by the status, and names the FITS target', async ({ page }) => {
    expect(await profile(page)).toBe('sun_ha')
    expect((await setProfile(page, 'comet')).status()).toBe(400)
    expect((await setProfile(page, 'moon')).ok()).toBe(true)
    expect(await profile(page)).toBe('moon')
  })

  test('the picker on the Camera tab changes the profile and the chip says so', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await page.getByRole('radio', { name: 'Moon' }).click()
    await expect.poll(() => profile(page)).toBe('moon')
    await expect(page.locator('.chip.profile')).toContainText('Moon')
    await page.getByRole('radio', { name: 'Sun, H-alpha' }).click()
    await expect.poll(() => profile(page)).toBe('sun_ha')
    await expect(page.locator('.chip.profile')).toHaveCount(0)
  })

  test('what does not apply is greyed with its reason, and is still on the page', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Record/ }).click()
    const prom = page.getByRole('radio', { name: '+ Prominence' })
    const hdr = page.getByRole('radio', { name: 'HDR lanes' })
    await expect(prom).toBeEnabled(); await expect(hdr).toBeEnabled()
    await setProfile(page, 'moon')
    await expect(prom).toBeDisabled(); await expect(hdr).toBeEnabled()
    await expect(page.getByText(/Prominence recordings pair a disc exposure/).first()).toBeVisible()
    await setProfile(page, 'planet')
    await expect(prom).toBeDisabled(); await expect(hdr).toBeDisabled() // greyed, not removed
    await expect(page.getByText(/Exposure lanes bracket a bright and a dark scene/)).toBeVisible()
    await page.getByRole('tab', { name: /^Track/ }).click()
    await expect(page.getByRole('radio', { name: 'Disc edge' })).toBeDisabled()
    await expect(page.getByRole('radio', { name: 'Feature' })).toBeEnabled()
    await expect(page.getByText(/there is no edge to fit/).first()).toBeVisible()
    await page.getByRole('tab', { name: /^More/ }).click()
    await page.getByText('Display', { exact: true }).first().click()
    await expect(page.getByRole('switch', { name: 'Prominence view (software coronagraph)' })).toBeDisabled()
    await expect(page.getByRole('switch', { name: 'Planet edge circle' })).toBeDisabled()
  })

  test('the Sun profile still has everything enabled (what the app did before profiles)', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Record/ }).click()
    await expect(page.getByRole('radio', { name: '+ Prominence' })).toBeEnabled()
    await expect(page.getByRole('radio', { name: 'HDR lanes' })).toBeEnabled()
    await page.getByRole('tab', { name: /^Track/ }).click()
    await expect(page.getByRole('radio', { name: 'Disc edge' })).toBeEnabled()
  })

  test('Go to the Moon offers the Moon profile, and never switches by itself', async ({ page }) => {
    await put(page, '/site', { latitude: 29.5, longitude: -96 })
    await page.goto('/')
    await page.getByRole('tab', { name: /^Aim/ }).click()
    const sel = page.getByLabel('Go to target')
    await expect(sel).toBeVisible()
    await sel.selectOption('moon')
    await expect(page.getByText(/Switch to the Moon profile|Switch to Moon/)).toBeVisible()
    expect(await profile(page)).toBe('sun_ha')
    await page.getByRole('button', { name: 'Switch to Moon' }).click()
    await expect.poll(() => profile(page)).toBe('moon')
  })
})

test.describe('colour histogram', () => {
  const status = async (page: Page) => (await (await page.request.get('/api/status')).json()).camera
  test('a colour camera shows R, G and B curves, with a clipping marker per channel; mono is unchanged', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    // mono (the default camera in these tests): one series, no per-channel fields
    await expect.poll(async () => (await status(page)).histogram.length).toBe(64)
    expect((await status(page)).histogram_rgb).toBeUndefined()
    await expect(page.locator('.hist svg')).toBeVisible()
    await expect(page.getByTestId('hist-rgb')).toHaveCount(0)

    await post(page, '/camera/close')
    await post(page, '/camera/open', { driver: 'simulator', id: 'sim_osc' })
    await expect.poll(async () => (await status(page))?.histogram_rgb?.length, { timeout: 10_000 }).toBe(3)
    const c = await status(page)
    expect(c.bayer).toBe('RGGB')
    for (const h of c.histogram_rgb) { expect(h.length).toBe(64); expect(h.reduce((a: number, b: number) => a + b, 0)).toBeCloseTo(1, 3) }
    await expect(page.getByTestId('hist-rgb')).toBeVisible()
    await expect(page.getByTestId('hist-rgb').locator('path')).toHaveCount(3)

    // over-expose: every channel clips and each gets its marker and a word
    await put(page, '/camera/control', { id: 'exposure_us', value: 1_000_000 })
    await put(page, '/camera/control', { id: 'gain', value: 400 })
    await expect.poll(async () => ((await status(page)).saturated_rgb ?? [0, 0, 0]).some((v: number) => v > 0.001), { timeout: 15_000 }).toBe(true)
    await expect(page.getByTestId('hist-rgb').locator('[data-clip]').first()).toBeVisible()
    await expect(page.getByRole('alert').filter({ hasText: /Clipping: .*%/ })).toBeVisible()
  })
})

test.describe('use profiles, stage 2: the engine knows the target', () => {
  const setProfile = (page: Page, id: string) => put(page, '/profile', { id })
  const status = async (page: Page) => (await (await page.request.get('/api/status')).json())
  test.afterEach(async ({ page }) => { await setProfile(page, 'sun_ha') })

  test('the flip follows the target: the Sun profile keeps the Sun numbers, the Moon its own meridian and lunar rate', async ({ page }) => {
    await put(page, '/site', { latitude: 29.5, longitude: -96 })
    await expect.poll(async () => (await status(page)).flip_target?.id).toBe('sun')
    let s = await status(page)
    expect(s.flip_target.minutes_to_meridian).toBe(s.sky_sun.minutes_to_meridian) // byte for byte what it was
    expect(s.flip_target.rate).toBe('solar')
    await setProfile(page, 'moon')
    await expect.poll(async () => (await status(page)).flip_target?.id).toBe('moon')
    s = await status(page)
    expect(s.target.name).toBe('Moon')
    expect(s.flip_target.rate).toBe('lunar')
    const moon = s.sky_targets.find((t: { id: string }) => t.id === 'moon')
    expect(s.flip_target.ra_hours).toBeCloseTo(moon.ra_hours, 3)
    // the Moon's hour angle grows at 0.966 of the clock's: minutes = hour angle / 0.966 (a star: / 1.0027)
    expect(Math.abs(s.flip_target.minutes_to_meridian + s.flip_target.hour_angle_h * 60 / 0.966)).toBeLessThan(0.04 * Math.abs(s.flip_target.minutes_to_meridian) + 0.5) // the topocentric Moon's rate changes through the day: a few per cent over hours
    // Other: nothing is followed
    await setProfile(page, 'other')
    await expect.poll(async () => (await status(page)).flip_target).toBeNull()
    expect((await status(page)).target).toBeNull()
  })

  test('the Planet profile follows the planet last gone to; the words and the radius are the Moon\'s and the planet\'s own', async ({ page }) => {
    await put(page, '/site', { latitude: 29.5, longitude: -96 })
    await setProfile(page, 'planet')
    await expect.poll(async () => (await status(page)).sky_targets?.length).toBeGreaterThan(0)
    await page.goto('/')
    await page.getByRole('tab', { name: /^Aim/ }).click()
    await page.getByText(/Meridian flip/i).first().click()
    expect((await post(page, '/mount/goto', { target: 'jupiter', confirm: true, allow_below_horizon: true })).ok()).toBe(true)
    await expect.poll(async () => (await status(page)).flip_target?.id, { timeout: 10_000 }).toBe('jupiter')
    await expect(page.getByText('Go to a planet once (Aim tab)')).toHaveCount(0) // the planet is known: the flip is not greyed
    expect((await status(page)).flip_target.rate).toBe('sidereal')
    await expect(page.getByText(/The Jupiter (crosses|crossed) the meridian/)).toBeVisible()
    // expected size: set the optics and compare the arcseconds
    await put(page, '/optics', { focal_length_mm: 1000, focal_multiplier: 1, pixel_size_um: 3.76, binning: 1 })
    await expect.poll(async () => (await status(page)).optics.target_radius_arcsec).toBeLessThan(30)
    await setProfile(page, 'moon')
    await expect.poll(async () => (await status(page)).optics.target_radius_arcsec).toBeGreaterThan(880)
    const o = (await status(page)).optics
    expect(o.expected_radius_px).toBeCloseTo(o.target_radius_arcsec / o.scale_arcsec_per_px, 3)
    await setProfile(page, 'sun_ha')
    await expect.poll(async () => (await status(page)).optics.target_radius_arcsec).toBeCloseTo((await status(page)).optics.solar_radius_arcsec, 3)
  })

  test('the Sun detector does not run for Other, and the engine says Moon where it speaks', async ({ page }) => {
    await expect.poll(async () => (await status(page)).sun, { timeout: 10_000 }).not.toBeNull()
    await setProfile(page, 'other')
    await expect.poll(async () => (await status(page)).sun, { timeout: 10_000 }).toBeNull()
    await setProfile(page, 'sun_ha')
    await expect.poll(async () => (await status(page)).sun, { timeout: 10_000 }).not.toBeNull()
  })

  test('a recording with no name is named after the Moon or the planet; the Sun profile adds nothing', async ({ page }) => {
    const fs = await import('node:fs')
    const names = () => {
      const out: string[] = []
      const walk = (d: string) => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { out.push(e.name); if (e.isDirectory()) walk(path.join(d, e.name)) } }
      walk(path.resolve('.e2e-data/data'))
      return out
    }
    await setProfile(page, 'moon')
    await expect.poll(async () => (await post(page, '/capture/sequence', { frames: 6, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
    await expect.poll(async () => (await status(page)).capture.state, { timeout: 20_000 }).toBe('idle')
    await expect.poll(() => names().some((n) => /^Moon/.test(n) || /Moon/.test(n))).toBe(true)
  })
})

test.describe('use profiles, stage 3: a planet and a phased Moon on the simulated camera', () => {
  const setProfile = (page: Page, id: string) => put(page, '/profile', { id })
  const status = async (page: Page) => (await (await page.request.get('/api/status')).json())
  const control = (page: Page, id: string, value: number) => put(page, '/camera/control', { id, value })
  test.afterEach(async ({ page }) => {
    await post(page, '/tracking/stop')
    await control(page, 'sim_scene', 0); await control(page, 'sim_seeing', 0); await control(page, 'sim_zoom', 1); await control(page, 'exposure_us', 2000)
    await setProfile(page, 'sun_ha')
  })

  test('Planet centroid: off with its reason on the Sun profile, the default on the Planet profile, and it finds and follows the planet', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Track/ }).click()
    // not a planet profile: greyed, on the page, with the reason
    await expect(page.getByRole('radio', { name: 'Planet centroid' })).toBeDisabled()
    await expect(page.getByText(/Planet centroid finds a small bright disc on a dark sky/)).toBeVisible()
    await post(page, '/mount/tracking', { enabled: true }) // the simulated sky drifts 15 px/s untracked
    await control(page, 'sim_scene', 3) // a new scene starts at the frame centre
    await control(page, 'sim_seeing', 2)
    await setProfile(page, 'planet')
    await expect(page.getByRole('radio', { name: 'Planet centroid' })).toBeEnabled()
    await expect(page.getByRole('radio', { name: 'Planet centroid' })).toBeChecked()
    await expect(page.getByRole('radio', { name: 'Disc edge' })).toBeDisabled() // there is no edge to fit
    // the engine finds the planet by itself (no Sun detector phantom), within a few pixels of the sim's planet at the frame centre
    await expect.poll(async () => { const s = (await status(page)).sun; return s ? Math.hypot(s.pos.x - 640, s.pos.y - 480) : 999 }, { timeout: 10_000 }).toBeLessThan(10)
    const sun = (await status(page)).sun
    expect(sun.radius).toBeLessThan(40)
    await page.waitForTimeout(800) // the engine keeps a frame for starting a tracker every half second: make sure it is one of the planet
    await page.getByRole('button', { name: 'Start tracking' }).click()
    await expect.poll(async () => (await status(page)).tracking?.mode, { timeout: 10_000 }).toBe('planet')
    await page.waitForTimeout(2500) // under 2 px of seeing it is followed, not lost
    const t = (await status(page)).tracking
    expect(t.lost).toBe(false)
    expect(Math.hypot(t.x - 640, t.y - 480)).toBeLessThan(10)
  })

  test('the Moon scene has a phase, and the planet profile judges clipping by the brightest part', async ({ page }) => {
    await post(page, '/mount/tracking', { enabled: true })
    await control(page, 'sim_scene', 2)
    await control(page, 'sim_phase', 0.5)
    await setProfile(page, 'moon')
    await expect.poll(async () => (await status(page)).sun?.radius ?? 0, { timeout: 10_000 }).toBeGreaterThan(250)
    await control(page, 'sim_phase', 0.25)
    await expect.poll(async () => (await status(page)).sun?.radius ?? 999, { timeout: 10_000 }).toBeLessThan(250) // the lit part: the terminator is not the limb
    // the planet: the mean and the saturated fraction cannot see it; the peak can
    await control(page, 'sim_scene', 3)
    await setProfile(page, 'planet')
    await expect.poll(async () => (await status(page)).camera.peak, { timeout: 10_000 }).toBeGreaterThan(0.2)
    expect((await status(page)).camera.mean).toBeLessThan(0.05)
    await page.goto('/')
    await expect(page.locator('.chip', { hasText: 'planet at full scale' })).toHaveCount(0)
    await control(page, 'exposure_us', 20000)
    await expect.poll(async () => (await status(page)).camera.peak, { timeout: 10_000 }).toBeGreaterThanOrEqual(0.98)
    await expect(page.locator('.chip', { hasText: 'planet at full scale' })).toBeVisible()
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await expect(page.getByRole('alert').filter({ hasText: /the planet\) is at/ })).toBeVisible()
  })
})

test.describe('optical setups: named trains that carry their profile', () => {
  const setupsOf = async (page: Page) => (await (await page.request.get('/api/optics/setups')).json()) as { active: string | null; modified: boolean; setups: { name: string; profile: string | null }[] }
  const status = async (page: Page) => (await (await page.request.get('/api/status')).json()) as { profile: string; optics: { focal_length_mm: number | null; focal_multiplier: number; setup: { active: string | null; modified: boolean } } }
  const del = (page: Page, name: string) => page.request.delete(`/api/optics/setups/${encodeURIComponent(name)}`)
  test.beforeEach(async ({ page }) => { for (const s of (await setupsOf(page)).setups) await del(page, s.name) })
  test.afterEach(async ({ page }) => {
    for (const s of (await setupsOf(page)).setups) await del(page, s.name)
    await put(page, '/optics', { focal_length_mm: null, focal_multiplier: 1, pixel_size_um: null, binning: 1 })
    await put(page, '/profile', { id: 'sun_ha' })
  })
  const openOptics = async (page: Page) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^More/ }).click()
    const d = page.locator('details', { has: page.locator('summary', { hasText: 'Optics' }) })
    if (!(await d.evaluate((e) => (e as HTMLDetailsElement).open))) await d.locator('summary').click()
    return page.getByRole('group', { name: 'Optical setups' })
  }
  const fill = async (page: Page, label: string, v: string) => page.getByLabel(label, { exact: true }).fill(v)

  test('the API: nothing exists at first; save from the current optics; validate; unique names; 404 for an unknown name', async ({ page }) => {
    expect((await setupsOf(page)).setups).toEqual([])
    await put(page, '/optics', { focal_length_mm: 400, focal_multiplier: 2, pixel_size_um: null, binning: 1 })
    expect((await put(page, '/optics/setups/Lunt%20%2B%20Barlow', {})).ok()).toBe(true)
    let l = await setupsOf(page)
    expect(l.active).toBe('Lunt + Barlow'); expect(l.modified).toBe(false)
    expect(l.setups[0]).toMatchObject({ name: 'Lunt + Barlow', profile: 'sun_ha', focal_length_mm: 400, focal_multiplier: 2 })
    expect((await put(page, '/optics/setups/bad', { profile: 'comet' })).status()).toBe(400)
    expect((await put(page, '/optics/setups/bad', { focal_length_mm: 5 })).status()).toBe(400)
    expect((await put(page, `/optics/setups/${'x'.repeat(41)}`, {})).status()).toBe(400)
    expect((await put(page, '/optics/setups/Lunt%20%2B%20Barlow', { notes: 'again' })).ok()).toBe(true)
    l = await setupsOf(page)
    expect(l.setups).toHaveLength(1)
    expect((await post(page, '/optics/setups/nope/apply')).status()).toBe(404)
    expect((await del(page, 'nope')).status()).toBe(404)
    expect((await status(page)).optics.setup).toEqual({ active: 'Lunt + Barlow', modified: false })
  })

  test('applying a setup sets the optics and switches to its profile; a missing flat is reported, not fatal; modified shows', async ({ page }) => {
    await put(page, '/optics', { focal_length_mm: 400, focal_multiplier: 2, pixel_size_um: null, binning: 1 })
    await put(page, '/profile', { id: 'sun_ha' })
    await put(page, '/optics/setups/Lunt', { flat: 'flats/not-there.png', aperture_mm: 50 })
    await put(page, '/optics/setups/C11', { profile: 'moon', focal_length_mm: 2800, focal_multiplier: 0.63, activate: false })
    const r = await (await post(page, '/optics/setups/C11/apply')).json()
    expect(r.after.profile).toBe('moon'); expect(r.before.profile).toBe('sun_ha')
    expect(r.after.focal_length_mm).toBe(2800)
    let s = await status(page)
    expect(s.profile).toBe('moon'); expect(s.optics.focal_multiplier).toBe(0.63); expect(s.optics.setup).toEqual({ active: 'C11', modified: false })
    const back = await (await post(page, '/optics/setups/Lunt/apply')).json()
    expect(back.not_restored[0]).toMatchObject({ what: 'flat', name: 'flats/not-there.png' })
    s = await status(page)
    expect(s.profile).toBe('sun_ha'); expect(s.optics.setup.modified).toBe(false)
    // a change of the profile or of the optics shows as modified
    await put(page, '/profile', { id: 'planet' })
    await expect.poll(async () => (await status(page)).optics.setup.modified).toBe(true)
    await post(page, '/optics/setups/Lunt/apply')
    await put(page, '/optics', { focal_length_mm: 500, focal_multiplier: 2, pixel_size_um: null, binning: 1 })
    await expect.poll(async () => (await status(page)).optics.setup.modified).toBe(true)
  })

  test('save as, apply (the profile follows), modified and revert, duplicate, delete with confirm, in the UI', async ({ page }) => {
    const g = await openOptics(page)
    await fill(page, 'Focal length (mm)', '400'); await fill(page, 'Barlow / reducer (×)', '2')
    await page.getByRole('button', { name: 'Save optics' }).click()
    await expect.poll(async () => (await status(page)).optics.focal_length_mm).toBe(400)
    // save as: profile of the train and aperture
    await g.getByLabel('Profile of this train').selectOption('sun_ha')
    await g.getByLabel('Aperture (mm)').fill('50')
    await expect(g.locator('.fratio')).toHaveText('f/16.0')
    await g.getByLabel('Name for a new setup').fill('  Lunt + Barlow  ')
    await g.getByRole('button', { name: 'Save as…' }).click()
    await expect(g.locator('.setup-name')).toHaveText('Lunt + Barlow')
    // a second train for the Moon
    await fill(page, 'Focal length (mm)', '2800'); await fill(page, 'Barlow / reducer (×)', '0.63')
    await g.getByLabel('Profile of this train').selectOption('moon')
    await g.getByLabel('Name for a new setup').fill('C11 + reducer')
    await g.getByRole('button', { name: 'Save as…' }).click()
    await expect(g.locator('.setup-name')).toHaveText('C11 + reducer')
    await expect(g.locator('.setup-badge')).toHaveCount(0)
    expect((await status(page)).profile).toBe('moon') // the train just saved is in use, with the profile chosen for it
    // apply the first: the result says what changed
    await g.getByLabel('Saved setups').selectOption('Lunt + Barlow')
    await g.getByRole('button', { name: /^Apply/ }).click()
    await expect(g.getByRole('status', { name: 'What applying changed' })).toContainText('Effective focal length')
    expect((await status(page)).profile).toBe('sun_ha')
    await expect(g.getByRole('status', { name: 'What applying changed' })).toContainText('Nothing was moved')
    await expect(g.locator('.setup-name')).toHaveText('Lunt + Barlow') // the status has caught up
    // then the Moon train: the profile follows
    await g.getByLabel('Saved setups').selectOption('C11 + reducer')
    await g.getByRole('button', { name: /^Apply/ }).click()
    await expect.poll(async () => (await status(page)).profile).toBe('moon')
    await expect(g.getByRole('status', { name: 'What applying changed' })).toContainText('Profile: Sun, H-alpha → Moon')
    expect((await status(page)).optics.setup).toEqual({ active: 'C11 + reducer', modified: false })
    // the name is next to the profile picker (Camera tab)
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await expect(page.getByLabel('Optical setup in use').first()).toHaveText('C11 + reducer · Moon')
    // changing the profile by hand marks the setup modified; Revert brings back the train's own profile
    await page.getByRole('radio', { name: 'Planet' }).click()
    await expect(page.getByLabel('Optical setup in use').first()).toContainText('(modified)')
    await page.getByRole('tab', { name: /^More/ }).click()
    await expect(g.locator('.setup-badge')).toHaveText('modified')
    await g.getByRole('button', { name: 'Revert' }).click()
    await expect.poll(async () => (await status(page)).profile).toBe('moon')
    await expect(g.locator('.setup-badge')).toHaveCount(0)
    // duplicate
    await g.getByLabel('Saved setups').selectOption('C11 + reducer')
    await g.getByRole('button', { name: 'Duplicate' }).click()
    await expect.poll(async () => (await setupsOf(page)).setups.map((s) => s.name)).toContain('C11 + reducer copy')
    expect((await setupsOf(page)).active).toBe('C11 + reducer')
    // delete asks first and says what is lost
    await g.getByLabel('Saved setups').selectOption('C11 + reducer copy')
    await g.getByRole('button', { name: 'Delete…' }).click()
    const ask = g.getByRole('alertdialog', { name: 'Delete the setup' })
    await expect(ask).toContainText('Forgotten')
    await ask.getByRole('button', { name: 'Keep it' }).click()
    expect((await setupsOf(page)).setups).toHaveLength(3)
    await g.getByRole('button', { name: 'Delete…' }).click()
    await ask.getByRole('button', { name: 'Delete setup' }).click()
    await expect.poll(async () => (await setupsOf(page)).setups.length).toBe(2)
  })

  test('no horizontal overflow and 44 px taps in the setup block', async ({ page }) => {
    await put(page, '/optics/setups/A', {})
    const g = await openOptics(page)
    await expect(g).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false)
    for (const b of await g.getByRole('button').all()) {
      const box = await b.boundingBox()
      if (box) expect(box.height, await b.innerText()).toBeGreaterThanOrEqual(43.5)
    }
  })

  test('a bad name is refused on screen, not silently', async ({ page }) => {
    const g = await openOptics(page)
    await g.getByLabel('Name for a new setup').fill('a'.repeat(41))
    await expect(g.getByRole('button', { name: 'Save as…' })).toBeDisabled()
    await expect(g.getByRole('note')).toContainText('1 to 40')
  })
})

test.describe('mount time and place (issue #104)', () => {
  test('differs -> chip -> send -> matches -> chip gone', async ({ page }) => {
    // the Pi's site, and a simulated mount that holds a clock 3 min 12 s ahead and a site 10 degrees away
    expect((await put(page, '/site', { latitude: 40, longitude: -105 })).ok()).toBe(true)
    expect((await post(page, '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 30, longitude_east_deg: -105 })).ok()).toBe(true)
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    // the chip is in the status strip on any tab, and jumps to the Aim tab
    const chip = page.locator('.chip.mountclock')
    await expect(chip).toBeVisible()
    await expect(chip).toContainText('Mount clock differs')
    expect((await chip.boundingBox())!.height).toBeGreaterThanOrEqual(43.5)
    await chip.click()
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('aria-selected', 'true')
    const disc = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: 'Mount time and place' }) })
    await expect(disc.locator('.disc-sum')).toContainText(/differs: 3 min (11|12|13) s, 10 degrees/) // the mount's clock is whole seconds
    await disc.locator('summary').click()
    await expect(disc).toContainText('Go to Sun is computed from the site and time the mount holds')
    await expect(disc.locator('.clocktable')).toContainText('30.000° N')
    await expect(disc.locator('.clocktable')).toContainText('40.000° N')
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false)
    const send = disc.getByRole('button', { name: 'Send site and time to the mount' })
    await expect(send).toBeEnabled()
    expect((await send.boundingBox())!.height).toBeGreaterThanOrEqual(43.5)
    await send.click()
    await expect(disc.locator('.disc-sum')).toContainText('matches')
    await expect(page.locator('.chip.mountclock')).toHaveCount(0)
    const st = (await (await page.request.get('/api/status')).json()).mount.site_time
    expect(st.ok).toBe(true)
    expect(Math.abs(st.time_diff_s)).toBeLessThan(5)
    expect(st.place_diff_deg).toBeLessThan(0.05)
  })

  test('the API: sync needs no confirmation, and is refused without a mount', async ({ page }) => {
    await put(page, '/site', { latitude: 40, longitude: -105 })
    expect((await post(page, '/mount/sync-site-time')).ok()).toBe(true)
    const st = (await (await page.request.get('/api/status')).json()).mount.site_time
    expect(st.ok).toBe(true)
    expect(st.read_at_unix).toBeGreaterThan(1_700_000_000)
    await post(page, '/mount/disconnect')
    expect((await post(page, '/mount/sync-site-time')).status()).toBe(400)
  })
})
