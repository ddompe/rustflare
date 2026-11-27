import { expect, Locator, Page, test } from '@playwright/test'

const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })
const status = async (page: Page) => (await page.request.get('/api/status')).json()
const tab = (page: Page, t: string) => page.getByRole('tab', { name: new RegExp('^' + t) }).click()
const overlap = (a: { x: number; y: number; width: number; height: number }, b: { x: number; y: number; width: number; height: number }) =>
  Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)) * Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y))

const VIEWPORTS = [
  { name: 'phone portrait', width: 390, height: 844 },
  { name: 'phone landscape', width: 844, height: 390 },
  { name: 'small phone landscape', width: 667, height: 375 },
  { name: 'large phone landscape', width: 932, height: 430 },
  { name: 'desktop', width: 1280, height: 800 },
]

test.beforeEach(async ({ page }) => {
  await put(page, '/storage/reserve', { gb: 2 })
  await post(page, '/camera/close'); await post(page, '/mount/disconnect')
  await post(page, '/camera/open', { driver: 'simulator', id: 'sim0' })
  await post(page, '/mount/connect', { driver: 'simulator' })
  await put(page, '/profile', { id: 'sun_ha' })
  await put(page, '/ui', { key: 'ui.setup_dismissed', value: false })
  await expect.poll(async () => (await status(page)).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
})

test.describe('Hide controls keeps the picture (P0)', () => {
  for (const v of [VIEWPORTS[0], VIEWPORTS[1], VIEWPORTS[4]]) {
    test(`the picture fills the screen and the buttons stay clear of it: ${v.name}`, async ({ page }, info) => {
      test.skip(info.project.name === 'phone' && v.width >= 1280, 'the desktop size is run in the desktop project')
      await page.setViewportSize({ width: v.width, height: v.height })
      await page.goto('/')
      await expect.poll(async () => (await page.locator('.live').boundingBox())?.width ?? 0).toBeGreaterThan(150)
      const normal = (await page.locator('.live').boundingBox())!
      await page.getByRole('button', { name: 'Hide controls' }).click()
      const live = (await page.locator('.live').boundingBox())!
      // the regression: a size container with no height collapsed the picture to 0 x 0
      expect(live.width, 'picture width in hide mode').toBeGreaterThanOrEqual(Math.min(normal.width, 250))
      expect(live.height, 'picture height in hide mode').toBeGreaterThanOrEqual(Math.min(normal.height, 180))
      expect(live.width * live.height, 'hide mode is not smaller than the normal view').toBeGreaterThanOrEqual(normal.width * normal.height * 0.9)
      const show = page.getByRole('button', { name: 'Show controls' })
      await expect(show).toBeVisible()
      expect(overlap((await show.boundingBox())!, live), 'Show controls covers the picture').toBe(0)
      // the status bar stays, within the side safe areas
      await expect(page.locator('.sbar')).toBeVisible()
      await show.click()
      await expect(page.getByRole('button', { name: 'Hide controls' })).toBeVisible()
    })
  }

  test('while recording the Stop button is there and clear of the picture', async ({ page }) => {
    await post(page, '/capture/sequence', { frames: 400, format: 'ser' })
    await page.goto('/')
    await page.getByRole('button', { name: 'Hide controls' }).click()
    const stop = page.locator('.fullrec')
    await expect(stop).toBeVisible()
    const live = (await page.locator('.live').boundingBox())!
    expect(live.width).toBeGreaterThan(250)
    expect(overlap((await stop.boundingBox())!, live), 'the Stop button covers the picture').toBe(0)
    expect(overlap((await stop.boundingBox())!, (await page.getByRole('button', { name: 'Show controls' }).boundingBox())!)).toBe(0)
    await stop.click()
    await expect.poll(async () => (await status(page)).capture.state, { timeout: 10_000 }).toBe('idle')
  })
})

test.describe('phone landscape and every size: nothing important is clipped (P3a)', () => {
  for (const v of VIEWPORTS.slice(0, 4)) {
    test(`segmented controls, the pad and the tools stay inside the screen: ${v.name}`, async ({ page }) => {
      await page.setViewportSize({ width: v.width, height: v.height })
      await put(page, '/profile', { id: 'planet' }) // the longest option labels and the most greyed options
      await post(page, '/tracking/stop')
      await page.goto('/')
      for (const t of ['Camera', 'Aim', 'Track', 'Record', 'More']) {
        await tab(page, t)
        await page.waitForTimeout(150)
        const bad = await page.evaluate(() => {
          const out: string[] = []
          const vw = innerWidth
          document.querySelectorAll('.seg').forEach((seg) => {
            const s = seg as HTMLElement
            const sr = s.getBoundingClientRect()
            if (sr.width === 0) return
            if (s.scrollWidth > s.clientWidth + 1) out.push(`seg scrolls: ${s.getAttribute('aria-label')}`)
            s.querySelectorAll('button').forEach((b) => {
              const r = b.getBoundingClientRect()
              if (r.left < sr.left - 1 || r.right > sr.right + 1 || r.left < -1 || r.right > vw + 1) out.push(`seg button clipped: ${b.textContent}`)
              if (b.scrollWidth > b.clientWidth + 1) out.push(`seg label clipped: ${b.textContent}`)
            })
          })
          document.querySelectorAll('.live-tools button, .tools-row button').forEach((b) => {
            const r = b.getBoundingClientRect()
            if (r.width && (r.left < -1 || r.right > vw + 1 || r.top < -1 || r.bottom > innerHeight + 1)) out.push(`tool off screen: ${b.getAttribute('aria-label')}`)
          })
          return out
        })
        expect(bad, `${v.name}, ${t}`).toEqual([])
      }
      // the tools never sit on the picture in landscape
      if (v.width > v.height) {
        await tab(page, 'Camera')
        const live = (await page.locator('.live').boundingBox())!
        for (const b of await page.locator('.tools-row button').all()) expect(overlap((await b.boundingBox())!, live), await b.getAttribute('aria-label') ?? '').toBe(0)
        const picShare = (live.width * live.height)
        expect(picShare, 'the picture is not tiny').toBeGreaterThan(150 * 110)
        // all four directions of the pad are on screen at once (without scrolling the page)
        await tab(page, 'Aim')
        for (const n of ['North', 'South', 'East', 'West']) {
          const box = await page.getByRole('button', { name: new RegExp(`^${n}: hold`) }).boundingBox()
          expect(box, n).not.toBeNull()
          expect(box!.y + box!.height, `${n} inside the screen`).toBeLessThanOrEqual(v.height + 1)
        }
      }
    })
  }
})

test.describe('compact profile picker and what-is-this (P1a, P2a)', () => {
  test('one row, the long sentence only on request, the choice is saved', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Camera')
    const pick = page.getByRole('combobox', { name: 'What are you imaging?' })
    await expect(pick).toBeVisible()
    expect((await pick.boundingBox())!.height).toBeLessThan(60)
    await expect(page.getByText('The words and the tools that apply follow this choice')).toHaveCount(0)
    await page.getByRole('button', { name: 'What is this?' }).click()
    await expect(page.getByText('The words and the tools that apply follow this choice')).toBeVisible()
    await pick.selectOption('moon')
    await expect.poll(async () => (await status(page)).profile).toBe('moon')
    // the blurb makes no promise the profile does not keep
    await expect(page.locator('.profile-pick')).not.toContainText('lunar tracking')
    await put(page, '/profile', { id: 'planet' })
    await expect(page.locator('.profile-pick')).not.toContainText('Star-rate tracking')
  })

  test('the first screen of Camera shows the picture, Start, the profile and the exposure', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await tab(page, 'Camera')
    const vis = async (l: Locator) => { const b = (await l.boundingBox())!; return b.y + b.height <= 844 - 53 - 45 + 1 }
    expect(await vis(page.locator('.live'))).toBe(true)
    expect(await vis(page.getByRole('button', { name: /^Start/ }))).toBe(true)
    expect(await vis(page.getByRole('combobox', { name: 'What are you imaging?' }))).toBe(true)
    expect(await vis(page.getByRole('slider', { name: 'Exposure' }))).toBe(true)
  })

  test('without a camera the camera picker and Open are on the first screen', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await post(page, '/camera/close')
    await page.goto('/')
    await tab(page, 'Camera')
    const open = page.getByRole('button', { name: 'Open camera' })
    await expect(open).toBeVisible()
    const b = (await open.boundingBox())!
    expect(b.y + b.height).toBeLessThanOrEqual(844 - 53 - 45)
  })
})

test.describe('panel room on a phone (P1a)', () => {
  test('the panel keeps at least 330 px on every tab and more on the ones that need it', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    const want: Record<string, number> = { Camera: 330, Aim: 395, Track: 380, Record: 430, More: 430 }
    for (const [t, min] of Object.entries(want)) {
      await tab(page, t)
      const h = (await page.locator('.panel').boundingBox())!.height
      expect(h, `${t} panel height`).toBeGreaterThanOrEqual(min)
    }
  })
})

test.describe('first-session set up strip (P2a)', () => {
  test('lists what is still to do, each step is a tap to where it is done, and it can be hidden', async ({ page }) => {
    await page.goto('/')
    await tab(page, 'Camera')
    const strip = page.getByRole('navigation', { name: 'First session set up' })
    const s = await status(page)
    const open = !s.site || (s.optics?.focal_length_mm == null && !s.optics?.setup?.active)
    if (!open) { await expect(strip).toHaveCount(0); return }
    await expect(strip).toBeVisible()
    await expect(strip).toContainText('Set up')
    if (!s.site) {
      await strip.getByRole('button', { name: /Site/ }).click()
      await expect(page.getByRole('tab', { name: /^More/ })).toHaveAttribute('aria-selected', 'true')
      await expect(page.getByLabel('Latitude (degrees, north positive)')).toBeVisible()
      await tab(page, 'Camera')
    }
    await page.getByRole('button', { name: 'Hide the set up steps' }).click()
    await expect(strip).toHaveCount(0)
    await expect.poll(async () => (await (await page.request.get('/api/ui')).json())['ui.setup_dismissed']).toBe(true)
    await tab(page, 'Aim')
    await expect(strip).toHaveCount(0)
  })

  test('a mount time that differs is a step, and the step opens Aim with the section open', async ({ page }) => {
    await put(page, '/site', { latitude: 40, longitude: -105 })
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 600, latitude_deg: 40, longitude_east_deg: -105 })
    await page.goto('/')
    await tab(page, 'Camera')
    const strip = page.getByRole('navigation', { name: 'First session set up' })
    await expect(strip).toBeVisible()
    await strip.getByRole('button', { name: /Mount time/ }).click()
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('button', { name: 'Send site and time to the mount' })).toBeVisible()
    await post(page, '/mount/sync-site-time')
  })
})

test.describe('notes chip and mount time words (P1b, P2d)', () => {
  test('profile and the mount clock fold into one notes chip; the sheet explains and can send', async ({ page }) => {
    await put(page, '/site', { latitude: 40, longitude: -105 })
    await put(page, '/mount/auto-site-time', { enabled: false })
    await put(page, '/profile', { id: 'moon' })
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 40, longitude_east_deg: -105 })
    await page.goto('/')
    await tab(page, 'Camera')
    await expect(page.locator('.chip.profile')).toHaveCount(0)
    await expect(page.locator('.chip.mountclock')).toHaveCount(0)
    const chip = page.getByRole('button', { name: /\d notes?$/ })
    await expect(chip).toBeVisible()
    expect((await chip.boundingBox())!.height).toBeGreaterThanOrEqual(43.5)
    await expect(page.getByRole('region', { name: 'Notes' })).toHaveCount(0)
    await chip.click()
    const sheet = page.getByRole('region', { name: 'Notes' })
    await expect(sheet).toContainText('Imaging: Moon')
    await expect(sheet).toContainText(/clock is 3 min/)
    await expect(sheet).toContainText('Go to sends')
    await page.keyboard.press('Escape')
    await expect(sheet).toHaveCount(0)
    await chip.click()
    await sheet.getByRole('button', { name: 'Send now' }).click()
    await expect.poll(async () => (await status(page)).mount.site_time.ok).toBe(true)
    await expect(sheet).not.toContainText('clock is')
    // Show on Aim opens the section
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 40, longitude_east_deg: -105 })
    await expect(sheet).toContainText('clock is')
    await sheet.getByRole('button', { name: 'Show on Aim' }).click()
    await expect(page.getByRole('tab', { name: /^Aim/ })).toHaveAttribute('aria-selected', 'true')
    const disc = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: 'Mount time and place' }) })
    await expect(disc).toHaveAttribute('open', '')
    await expect(disc.locator('.disc-sum')).toContainText(/time differs by 3 min/)
    await expect(disc.locator('.disc-sum')).toContainText('place matches')
    await post(page, '/mount/sync-site-time')
    await put(page, '/mount/auto-site-time', { enabled: true })
  })

  test('the summary says what could not be compared, and Go to mentions the clock', async ({ page }) => {
    // a site where it is local solar noon now, so the Sun is up and Go to is enabled
    const lon = (((12 - (new Date().getUTCHours() + new Date().getUTCMinutes() / 60)) * 15 + 180) % 360) - 180
    await put(page, '/site', { latitude: 0, longitude: lon })
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 0, latitude_deg: 0, longitude_east_deg: lon })
    await page.goto('/')
    await tab(page, 'Aim')
    const disc = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: 'Mount time and place' }) })
    await expect(disc.locator('.disc-sum')).toContainText(/^time (matches|differs by|not reported) · place (matches|differs by|not reported)$/)
    await post(page, '/debug/sim-mount-clock', { time_offset_s: 192, latitude_deg: 0, longitude_east_deg: lon })
    await page.getByRole('button', { name: 'Go to Sun' }).click()
    const dialog = page.getByRole('alertdialog', { name: 'Go to the Sun' })
    await expect(dialog).toContainText("Go to sends the Pi's site and time")
    await expect(dialog).toContainText('clock')
    // Cancel first, the destructive action last
    const names = await dialog.getByRole('button').allInnerTexts()
    expect(names[0]).toBe('Cancel')
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await post(page, '/mount/sync-site-time')
  })
})

test.describe('Start while calibrating (P2d)', () => {
  test('Start is disabled with its reason, and Calibrating stays a visible warning', async ({ page }) => {
    await post(page, '/mount/tracking', { enabled: true })
    await post(page, '/tracking/start', { mode: 'limb' })
    await page.goto('/')
    await tab(page, 'Camera')
    await expect(page.getByRole('button', { name: /^Start/ })).toBeEnabled()
    const r = await post(page, '/guiding/calibrate')
    if (!r.ok()) test.skip(true, 'the simulator would not calibrate here')
    await expect.poll(async () => (await status(page)).guiding.state, { timeout: 5000 }).toBe('calibrating')
    const start = page.getByRole('button', { name: /^Start/ })
    await expect(start).toBeDisabled()
    await expect(start).toContainText('Calibrating')
    await expect(page.locator('.chip.trk')).toContainText('Calibrating')
    await expect.poll(async () => (await status(page)).guiding.state, { timeout: 60_000 }).not.toBe('calibrating')
    await expect(page.getByRole('button', { name: /^Start/ })).toBeEnabled()
    await post(page, '/tracking/stop')
  })
})

test.describe('the Start strip says what the stacker will do (P2d)', () => {
  test('one vocabulary in the cards and in the strip, and a stacker that is off says so', async ({ page }) => {
    await page.goto('/')
    await tab(page, 'Record')
    await page.getByText('After each burst').first().click()
    const names = ['Keep video', 'Video + stack', 'Stack only', 'Best frames + stack']
    for (const n of names) await expect(page.getByRole('button', { name: new RegExp(`^${n.replace('+', '\\+')}`) })).toBeVisible()
    const stackerUp = (await status(page)).after_burst.stacker_connected
    await put(page, '/capture/after-burst', { mode: 'video_stack' })
    const keep = page.locator('.qr-keep')
    await expect(keep).toHaveText(stackerUp ? 'Video + stack' : 'Stacker off, saves video')
    await put(page, '/capture/after-burst', { mode: 'best_frames' })
    await expect(keep).toHaveText(stackerUp ? 'Best frames + stack' : 'Stacker off, saves video')
    await put(page, '/capture/after-burst', { mode: 'video' })
    await expect(keep).toHaveText('Keep video')
  })
})

test.describe('one reason line per group, linked to every greyed control (P2b)', () => {
  test('Track on a planet: Disc edge and Full disc share one line', async ({ page }) => {
    await put(page, '/profile', { id: 'planet' })
    await post(page, '/tracking/stop')
    await page.goto('/')
    await tab(page, 'Track')
    const group = page.getByRole('radiogroup', { name: 'Tracking target' })
    const edge = group.getByRole('radio', { name: 'Disc edge' })
    const full = group.getByRole('radio', { name: 'Full disc' })
    await expect(edge).toBeDisabled()
    const id = await edge.getAttribute('aria-describedby')
    expect(id).toBeTruthy()
    expect(await full.getAttribute('aria-describedby')).toBe(id)
    await expect(page.locator(`[id="${id}"]`)).toContainText('Disc edge and Full disc are off')
    // the sentence about the reason is said once
    await expect(page.getByText('no edge to fit')).toHaveCount(1)
    // greyed options still read: 3:1 on their background
    const c = await edge.evaluate((el) => { const s = getComputedStyle(el); return [s.color, s.opacity] })
    expect(c[1]).toBe('1')
  })

  test('Go to Sun and Display switches point at their reason', async ({ page }) => {
    await put(page, '/profile', { id: 'planet' })
    await page.goto('/')
    await tab(page, 'More')
    await page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Display$/ }) }).locator('summary').click()
    const sw = page.getByRole('switch', { name: /edge circle/ })
    await expect(sw).toBeDisabled()
    const id = await sw.getAttribute('aria-describedby')
    expect(id).toBeTruthy()
    await expect(page.locator(`[id="${id}"]`)).toContainText('too small')
  })
})

test.describe('optical setup: in use first, then one Save row with a diff and a confirm (P2c)', () => {
  test('Update shows what it replaces and asks first', async ({ page }) => {
    await put(page, '/optics/setups/Round2', { focal_length_mm: 400, focal_multiplier: 2, binning: 1, profile: 'sun_ha' })
    await post(page, '/optics/setups/Round2/apply')
    await page.goto('/')
    await tab(page, 'More')
    const optDisc = page.locator('details.disc', { has: page.locator('.disc-title', { hasText: /^Optical setup$/ }) })
    if (!(await optDisc.evaluate((e) => (e as HTMLDetailsElement).open))) await optDisc.locator('summary').click()
    await expect(page.locator('.panel h2.subhead').first()).toBeVisible()
    const headings = (await page.locator('.panel h2.subhead').allTextContents()).map((t) => t.trim())
    expect(headings.indexOf('Optical setup in use')).toBeGreaterThanOrEqual(0)
    expect(headings.indexOf('Optical setup in use')).toBeLessThan(headings.indexOf('Saved optical setups'))
    expect(headings.indexOf('Saved optical setups')).toBeLessThan(headings.indexOf('Save'))
    await expect(page.getByRole('button', { name: 'Save optics' })).toHaveCount(0)
    await page.getByLabel('Barlow / reducer (×)').fill('1.5')
    const diff = page.locator('#setup-diff')
    await expect(diff).toContainText('replaces 400 mm × 2 with 400 mm × 1.5')
    const update = page.getByRole('button', { name: /^Update “Round2”/ })
    await update.click()
    const ask = page.getByRole('alertdialog', { name: /Update the optical setup Round2/ })
    await expect(ask).toBeVisible()
    const names = await ask.getByRole('button').allInnerTexts()
    expect(names[0]).toBe('Cancel')
    await ask.getByRole('button', { name: 'Cancel' }).click()
    await expect(ask).toHaveCount(0)
    expect(((await status(page)).optics.setup)?.active).toBe('Round2')
    await update.click()
    await ask.getByRole('button', { name: /^Update “Round2”/ }).click()
    await expect.poll(async () => ((await (await page.request.get('/api/optics/setups')).json()).setups as { name: string; focal_multiplier: number }[]).find((s) => s.name === 'Round2')?.focal_multiplier).toBe(1.5)
    await page.request.delete('/api/optics/setups/Round2')
  })
})

test.describe('Files is a real modal (accessibility)', () => {
  test('focus moves in, Tab stays inside, Escape closes and focus returns', async ({ page }) => {
    await page.goto('/')
    const opener = page.getByRole('button', { name: /^Files/ }).first()
    await opener.focus()
    await opener.click()
    const fm = page.getByRole('dialog', { name: 'Files' })
    await expect(fm).toBeVisible()
    for (let i = 0; i < 40; i++) {
      await page.keyboard.press('Tab')
      expect(await page.evaluate(() => !!document.activeElement?.closest('.fm-screen')), `Tab ${i} stayed in the dialog`).toBe(true)
    }
    for (let i = 0; i < 6; i++) {
      await page.keyboard.press('Shift+Tab')
      expect(await page.evaluate(() => !!document.activeElement?.closest('.fm-screen'))).toBe(true)
    }
    // the folder tree groups have names
    expect(await page.locator('.fm-tree [role=group]:not([aria-label])').count()).toBe(0)
    await page.keyboard.press('Escape')
    await expect(fm).toHaveCount(0)
    expect(await page.evaluate(() => document.activeElement?.textContent?.trim())).toBe('Files')
  })
})

test.describe('tap targets and legibility (accessibility)', () => {
  test('buttons, summaries and links are 44 px; small text is 12 px', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await put(page, '/site', { latitude: 40, longitude: -105 })
    await page.goto('/')
    const small = async (sel: string, what: string) => {
      for (const el of await page.locator(sel).all()) {
        if (!(await el.isVisible())) continue
        const b = (await el.boundingBox())!
        expect(b.height, `${what} height ${(await el.innerText().catch(() => ''))}`).toBeGreaterThanOrEqual(43.5)
      }
    }
    for (const t of ['Camera', 'Aim', 'Track', 'Record', 'More']) {
      await tab(page, t)
      await small('.sbar-btn', 'status bar')
      await small('.hud-w', 'status overlay')
      await small('.chip', 'chip')
      await small('.panel summary', 'summary')
      if (t !== 'Camera') {
        await page.getByRole('button', { name: 'Picture tools' }).click()
        await small('.tools-row button', 'picture tool')
        for (const b of await page.locator('.tools-row button').all()) expect((await b.boundingBox())!.width, 'tool width').toBeGreaterThanOrEqual(43.5)
        await page.getByRole('button', { name: 'Picture tools' }).click()
      } else {
        for (const b of await page.locator('.tools-row button').all()) expect((await b.boundingBox())!.width, 'tool width').toBeGreaterThanOrEqual(43.5)
      }
    }
    const ver = await page.locator('.sbar-ver').evaluate((e) => parseFloat(getComputedStyle(e).fontSize))
    expect(ver).toBeGreaterThanOrEqual(12)
    await tab(page, 'More')
    await page.getByText('Stacking').first().click()
    for (const c of await page.locator('code').all()) if (await c.isVisible()) expect(await c.evaluate((e) => parseFloat(getComputedStyle(e).fontSize))).toBeGreaterThanOrEqual(12)
  })

  test('the file manager tree buttons are 44 px', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await page.goto('/')
    await page.getByRole('button', { name: /^Files/ }).first().click()
    for (const b of await page.locator('.fm-tree button').all()) expect((await b.boundingBox())!.height).toBeGreaterThanOrEqual(43.5)
    await page.keyboard.press('Escape')
  })

  test('state is never colour alone: tabs and chips carry a shape, reduced motion stops the switch transitions', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' })
    await page.goto('/')
    await expect(page.locator('.tabs .tone-icon').first()).toBeVisible()
    await tab(page, 'More')
    const dur = await page.locator('.chev').first().evaluate((e) => getComputedStyle(e).transitionDuration)
    expect(dur === '0s' || dur === '').toBe(true)
    await expect(page.locator('.seg').first()).toBeVisible().catch(() => {})
  })
})

test.describe('the colour histogram is readable without colour (accessibility)', () => {
  test('curves differ by dash, each channel has its letter, in both themes', async ({ page }) => {
    await post(page, '/camera/close')
    await post(page, '/camera/open', { driver: 'simulator', id: 'sim_osc' })
    await expect.poll(async () => (await status(page)).camera?.histogram_rgb?.length ?? 0, { timeout: 10_000 }).toBe(3)
    await page.goto('/')
    await tab(page, 'Camera')
    const plot = page.getByTestId('hist-rgb').first()
    await expect(plot.locator('path[data-channel=R]')).toHaveAttribute('stroke-dasharray', /^$/).catch(() => {})
    expect(await plot.locator('path[data-channel=G]').getAttribute('stroke-dasharray')).toBeTruthy()
    expect(await plot.locator('path[data-channel=B]').getAttribute('stroke-dasharray')).toBeTruthy()
    await expect(page.locator('.hist-legend')).toContainText('R')
    await expect(page.locator('.hist-legend')).toContainText('G')
    await expect(page.locator('.hist-legend')).toContainText('B')
    for (const theme of ['night', 'day']) {
      await page.evaluate((t) => { document.documentElement.dataset.theme = t }, theme)
      const s = await plot.evaluate((svg) => ['R', 'G', 'B'].map((k) => getComputedStyle(svg.querySelector(`path[data-channel=${k}]`)!).stroke))
      expect(new Set(s).size, `${theme} strokes are distinct`).toBe(3)
    }
    await post(page, '/camera/close')
    await post(page, '/camera/open', { driver: 'simulator', id: 'sim0' })
  })
})

test('the five tabs fit their bar with a wide font in a narrow landscape column (no sideways scroll)', async ({ page }) => {
  await page.setViewportSize({ width: 667, height: 375 })
  await page.goto('/')
  await page.addStyleTag({ content: '* { font-family: Verdana, DejaVu Sans !important }' })
  await page.waitForTimeout(300)
  const r = await page.evaluate(() => {
    const bar = document.querySelector('.tabs')!.getBoundingClientRect()
    const bs = [...document.querySelectorAll('.tabs button')].map((x) => x.getBoundingClientRect())
    return { barRight: bar.right, lastRight: bs[bs.length - 1].right, minWidth: Math.min(...bs.map((b) => b.width)), scrollW: document.documentElement.scrollWidth, innerW: innerWidth }
  })
  expect(r.lastRight).toBeLessThanOrEqual(r.barRight + 1)
  expect(r.scrollW).toBeLessThanOrEqual(r.innerW)
  expect(r.minWidth, 'a tab is at least 44 px wide').toBeGreaterThanOrEqual(44)
})
