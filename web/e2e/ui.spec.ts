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
  await page.getByText('Captures').click()
  const row = page.locator('.fm-row', { hasText: 'e2e' })
  await expect(row).toBeVisible()
  await row.getByRole('button', { name: 'Check' }).click()
  await expect(row).toContainText('10/10 frames')
  await expect(row).not.toContainText('Mostly dark')
  await row.getByRole('button', { name: 'Delete' }).click()
  await row.getByRole('button', { name: 'Delete' }).last().click()
  await expect(page.locator('.fm-row', { hasText: 'e2e' })).toHaveCount(0)
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
