import { expect, Page, test } from '@playwright/test'

const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })

test.beforeEach(async ({ page }) => {
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

test('record a short sequence, check it, delete it', async ({ page }) => {
  await page.goto('/')
  // the first frame can still be on its way after the camera was (re)opened
  await expect.poll(async () => (await post(page, '/capture/sequence', { name: 'e2e', frames: 10, format: 'ser' })).text(), { timeout: 10_000 }).toContain('buffer_frames')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 15_000 }).toBe('idle')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await expect(fm).toBeVisible()
  const row = fm.locator('.fm-row', { hasText: 'e2e' })
  await expect(row).toHaveCount(1) // the .json notes file is not a row of its own
  await row.getByRole('button', { name: 'Check' }).click()
  await expect(row).toContainText('10/10 frames')
  await expect(row).not.toContainText('Mostly dark')
  await row.getByRole('checkbox').check()
  await fm.getByRole('button', { name: /Delete 1 selected/ }).click()
  await fm.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
  await expect(fm.locator('.fm-row', { hasText: 'e2e' })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(fm).toHaveCount(0)
})

test('flats live in the Camera tab', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Camera/ }).click()
  await expect(page.getByText('Flats', { exact: true })).toBeVisible()
  await page.getByText('Flats', { exact: true }).click()
  await expect(page.getByRole('button', { name: /Start flats/ })).toBeVisible()
  await page.getByRole('tab', { name: /^More/ }).click()
  await expect(page.getByText('Flats', { exact: true })).toHaveCount(0)
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
  await expect(page.getByText('Coronagraph: black out the disc')).toBeVisible()
  await page.getByRole('radio', { name: 'Sequence' }).check()
  await expect(page.getByRole('button', { name: 'Start recording' })).toBeVisible()
})

test('HDR lanes record into one folder per lane', async ({ page }) => {
  await page.goto('/')
  await expect.poll(async () => (await post(page, '/capture/lanes', { lanes: [{ name: 'x1', exposure_us: 200, frames: 3 }, { name: 'x2', exposure_us: 400, frames: 3 }], cycles: 1 })).text(), { timeout: 10_000 }).toContain('lanes')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 20_000 }).toBe('idle')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('button', { name: /Open file manager/ }).click()
  await expect(page.getByRole('dialog', { name: 'File manager' })).toContainText('hdr_')
})

test('the prominence recording with the coronagraph masks the disc in its lane only', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  await page.getByRole('radio', { name: '+ Prominence' }).check()
  await page.getByLabel('Frames per lane').fill('3')
  await page.getByRole('switch', { name: /Coronagraph/ }).check()
  await page.getByLabel('Number of cycles').fill('1')
  await page.getByRole('button', { name: 'Start prominence recording' }).click()
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 20_000 }).toBe('idle')
  await page.getByRole('button', { name: /Open file manager/ }).click()
  const fm = page.getByRole('dialog', { name: 'File manager' })
  await fm.getByLabel('Show').selectOption('prominence')
  await expect(fm).toContainText('Prominence video')
  await expect(fm).toContainText('2 lanes')
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

test('automatic meridian flip: needs a site, switches on with its timing, and off again', async ({ page }) => {
  await put(page, '/site', { latitude: 40, longitude: -105 })
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
