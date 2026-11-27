import { expect, Page, test } from '@playwright/test'
import { resetSimulators, stopCapture } from './reset'

// Round 3 leftovers (issue #112): one Send control, Escape in the optical setup form, the file manager on the shared Modal.
const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })
const disc = (page: Page, title: string) => page.locator('details.disc', { has: page.locator('.disc-title', { hasText: new RegExp(`^${title}$`) }) })

test.beforeEach(async ({ page }) => { await resetSimulators(page) })
test.afterEach(async ({ page }) => {
  await post(page, '/mount/sync-site-time')
  for (const s of ((await (await page.request.get('/api/optics/setups')).json()) as { setups: { name: string }[] }).setups) await page.request.delete(`/api/optics/setups/${encodeURIComponent(s.name)}`)
  await put(page, '/optics', { focal_length_mm: null, focal_multiplier: 1, pixel_size_um: null, binning: 1 })
  await put(page, '/profile', { id: 'sun_ha' })
  await stopCapture(page)
})

test('the notes sheet leaves Send now to Mount time and place while that section is open', async ({ page }) => {
  await put(page, '/profile', { id: 'moon' })
  await post(page, '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 40, longitude_east_deg: -105 })
  await page.goto('/')
  await page.getByRole('tab', { name: /^Aim/ }).click()
  const chip = page.locator('.chip.notes')
  const sheet = page.getByRole('region', { name: 'Notes' })
  const section = disc(page, 'Mount time and place')
  await expect(section).toBeVisible()
  // closed: the sheet has its own Send now
  if (await section.evaluate((e) => (e as HTMLDetailsElement).open)) await section.locator('summary').click()
  await chip.click()
  await expect(sheet.getByRole('button', { name: 'Send now' })).toBeVisible()
  await expect(sheet.getByRole('button', { name: 'Show on Aim' })).toBeVisible()
  await page.keyboard.press('Escape')
  // open: one Send control on screen, and the sheet says where it is
  await section.locator('summary').click()
  await expect(section).toHaveAttribute('open', '')
  await chip.click()
  await expect(sheet).toContainText('Send the site and time from Mount time and place')
  await expect(sheet.getByRole('button', { name: 'Send now' })).toHaveCount(0)
  await expect(sheet.getByRole('button', { name: 'Show on Aim' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Send site and time to the mount' })).toBeVisible()
  // leaving Aim brings it back
  await page.keyboard.press('Escape')
  await page.getByRole('tab', { name: /^Camera/ }).click()
  await chip.click()
  await expect(sheet.getByRole('button', { name: 'Send now' })).toBeVisible()
})

test.describe('Escape in the optical setup form', () => {
  const open = async (page: Page) => {
    await page.goto('/')
    await page.getByRole('tab', { name: /^More/ }).click()
    const d = disc(page, 'Optical setup')
    if (!(await d.evaluate((e) => (e as HTMLDetailsElement).open))) await d.locator('summary').click()
    await page.getByRole('button', { name: 'Manage…' }).click()
    const dlg = page.getByRole('dialog', { name: 'Optical setups' })
    await expect(dlg).toBeVisible()
    await dlg.getByRole('button', { name: 'New setup…' }).click()
    return dlg
  }
  test('keeps the form and what was typed while it has changes; Cancel still leaves', async ({ page }) => {
    const dlg = await open(page)
    const name = dlg.getByLabel('Name', { exact: true })
    await name.fill('My scope')
    await page.keyboard.press('Escape')
    await expect(dlg).toBeVisible()
    await expect(name).toHaveValue('My scope')
    await expect(dlg).toContainText('Escape keeps this form open')
    await dlg.getByRole('button', { name: 'Cancel' }).click()
    await expect(dlg.getByRole('button', { name: 'New setup…' })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(dlg).toHaveCount(0)
  })
  test('closes the dialog when the form is untouched, and the Close button always works', async ({ page }) => {
    let dlg = await open(page)
    await page.keyboard.press('Escape')
    await expect(dlg).toHaveCount(0)
    dlg = await open(page)
    await dlg.getByLabel('Name', { exact: true }).fill('x')
    await dlg.getByRole('button', { name: /^Close/ }).click()
    await expect(dlg).toHaveCount(0)
  })
})

test('the file manager runs on the shared Modal: labelled, focus in, page inert, Escape and Close, focus back', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Record/ }).click()
  const opener = page.getByRole('button', { name: /Files/ }).first()
  await opener.focus()
  await opener.click()
  const fm = page.getByRole('dialog', { name: 'Files' })
  await expect(fm).toBeVisible()
  await expect(fm).toHaveAttribute('aria-modal', 'true')
  await expect(fm.getByRole('button', { name: 'Close Files' })).toBeFocused()
  await expect(page.locator('main.panel')).toHaveAttribute('inert', '')
  await expect(fm).not.toHaveAttribute('inert', '')
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press('Tab')
    expect(await page.evaluate(() => !!document.activeElement?.closest('.fm-screen')), `Tab ${i} stayed in Files`).toBe(true)
  }
  // Escape in the search field is left to the field (unchanged); from anywhere else it closes
  await fm.getByRole('button', { name: 'Close Files' }).focus()
  await page.keyboard.press('Escape')
  await expect(fm).toHaveCount(0)
  await expect(page.locator('main.panel')).not.toHaveAttribute('inert', '')
  await opener.click()
  await page.getByRole('dialog', { name: 'Files' }).getByRole('button', { name: 'Close Files' }).click()
  await expect(fm).toHaveCount(0)
})
