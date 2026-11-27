import { expect, Locator, Page, test } from '@playwright/test'
import { resetSimulators, stopCapture } from './reset'

// Choosing a region of interest by dragging on the picture. The simulator camera is 1280 x 960 with steps of 2 (corner), 4 (width) and 2 (height),
// minimum 64 x 64. Everything asserts on geometry and on numbers, never on pixels: the simulated picture can be black.
const SW = 1280, SH = 960

const post = (page: Page, path: string, body: unknown = {}) => page.request.post(`/api${path}`, { data: body })
const put = (page: Page, path: string, body: unknown) => page.request.put(`/api${path}`, { data: body })
const status = async (page: Page) => (await page.request.get('/api/status')).json()
const WIDE = '* { font-family: Verdana, "DejaVu Sans", sans-serif !important; letter-spacing: .02em !important }'

test.afterEach(async ({ page }) => { await stopCapture(page) })

test.beforeEach(async ({ page }, info) => {
  test.skip(info.project.name !== 'phone', 'sizes are set in each test; one project is enough')
  await resetSimulators(page)
  for (const [key, value] of [['ui.setup_dismissed', true], ['ui.profile_confirmed', true], ['ui.theme', 'night']] as const) await put(page, '/ui', { key, value })
  await put(page, '/camera/roi', { fraction: 1 })
  await expect.poll(async () => (await status(page)).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
})

const fields = (page: Page) => ({ x: page.locator('#region-x'), y: page.locator('#region-y'), w: page.locator('#region-w'), h: page.locator('#region-h') })
const num = async (l: Locator) => Number(await l.inputValue())
const rect = async (page: Page) => { const f = fields(page); return { x: await num(f.x), y: await num(f.y), w: await num(f.w), h: await num(f.h) } }
const pic = async (page: Page) => (await page.getByTestId('region-select').boundingBox())!
const toScreen = async (page: Page, sx: number, sy: number) => { const b = await pic(page); return { x: b.x + (sx / SW) * b.width, y: b.y + (sy / SH) * b.height } }
const roiNow = async (page: Page) => (await status(page)).roi

async function enter(page: Page, via: 'tools' | 'camera' = 'tools') {
  await page.goto('/')
  await page.getByRole('tab', { name: /^Camera/ }).click()
  if (via === 'tools') {
    await page.getByRole('button', { name: 'Picture tools' }).click()
    await page.getByRole('button', { name: 'Region…' }).click()
  } else {
    await page.getByText('Frame size').first().click()
    await page.getByRole('button', { name: 'Region…' }).click()
  }
  await expect(page.getByTestId('region-select')).toBeVisible()
  await expect(page.getByTestId('region-panel')).toBeVisible()
}

/** Drags a handle (or the body) by a number of screen pixels with real pointer events. */
async function dragBy(page: Page, testid: string, dx: number, dy: number) {
  const el = page.getByTestId(testid)
  const b = (await el.boundingBox())!
  const cx = b.x + b.width / 2, cy = b.y + b.height / 2
  await page.mouse.move(cx, cy)
  await page.mouse.down()
  await page.mouse.move(cx + dx / 2, cy + dy / 2, { steps: 4 })
  await page.mouse.move(cx + dx, cy + dy, { steps: 4 })
  await page.mouse.up()
}

const setLock = (page: Page, label: string) => page.getByLabel('Aspect lock').selectOption({ label })
const near = (a: number, b: number, tol: number) => Math.abs(a - b) <= tol

test.describe('selection mode', () => {
  test('entering shows the whole sensor with the selection on top, and the numbers say where it is', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await put(page, '/camera/roi', { fraction: 0.5 })
    await expect.poll(async () => (await status(page)).camera.width).toBe(640)
    await enter(page)
    await expect.poll(async () => { const s = await status(page); return [s.camera.width, s.camera.height, s.roi.width, s.roi.height] }).toEqual([SW, SH, SW, SH])
    await expect(page.getByTestId('size-label')).toHaveText('1280×960')
    expect(await rect(page)).toEqual({ x: 320, y: 240, w: 640, h: 480 }) // the earlier (centred half) region is where the selection starts
    await expect(page.getByTestId('region-result')).toContainText('Will read 640 × 480 at 320, 240; about 4× less data per frame')
    for (const h of ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']) {
      const b = (await page.getByTestId(`handle-${h}`).boundingBox())!
      expect(b.width).toBeGreaterThanOrEqual(44); expect(b.height).toBeGreaterThanOrEqual(44)
    }
    // the previous region came from the engine, so Cancel can put it back
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 320, y: 240, width: 640, height: 480 })
  })

  test('from the Camera tab too, and from a full-frame start the selection begins as the centred half', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await enter(page, 'camera')
    expect(await rect(page)).toEqual({ x: 320, y: 240, w: 640, h: 480 })
    await expect(page.getByTestId('region-label')).toHaveText('640 × 480 at 320, 240')
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByTestId('region-select')).toHaveCount(0)
    expect((await roiNow(page)).width).toBe(SW)
  })

  test('dragging the body moves the region and the fields follow; it stops at the edges', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await enter(page)
    const b = await pic(page)
    const perPx = SW / b.width
    await dragBy(page, 'region-body', 40, 20)
    let r = await rect(page)
    expect(near(r.x, 320 + 40 * perPx, 4)).toBe(true)
    expect(near(r.y, 240 + 20 * (SH / b.height), 4)).toBe(true)
    expect([r.w, r.h]).toEqual([640, 480])
    expect(r.x % 2 + r.y % 2).toBe(0)
    await dragBy(page, 'region-body', 900, 900)
    r = await rect(page)
    expect(r).toEqual({ x: SW - 640, y: SH - 480, w: 640, h: 480 })
    await dragBy(page, 'region-body', -900, -900)
    expect(await rect(page)).toEqual({ x: 0, y: 0, w: 640, h: 480 })
  })

  test('resizing by a corner with Free, 1:1, 16:9 and the sensor shape', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await enter(page)
    const b = await pic(page)
    const perPx = SW / b.width
    // Free: both sides move on their own, the opposite corner stays
    await dragBy(page, 'handle-se', 60, 30)
    let r = await rect(page)
    expect([r.x, r.y]).toEqual([320, 240])
    expect(near(r.w, 640 + 60 * perPx, 8)).toBe(true)
    expect(near(r.h, 480 + 30 * perPx, 8)).toBe(true)
    expect(Math.abs(r.w / r.h - 640 / 480)).toBeGreaterThan(0.005) // the shape changed

    for (const [label, ratio] of [['1:1', 1], ['16:9', 16 / 9], ['Sensor shape', SW / SH], ['4:3', 4 / 3]] as const) {
      await page.getByRole('button', { name: 'Full frame' }).click() // a clean start each time
      await setLock(page, label)
      r = await rect(page)
      expect(Math.abs(r.w / r.h - ratio) / ratio, `${label} after choosing it`).toBeLessThan(0.01)
      await page.getByRole('button', { name: 'Centre' }).click()
      // shrink from the top-left corner, then grow from the bottom-right one: the shape holds every time
      await dragBy(page, 'handle-nw', 50, 10)
      r = await rect(page)
      expect(Math.abs(r.w / r.h - ratio) / ratio, `${label} after nw`).toBeLessThan(0.012)
      const before = r
      await dragBy(page, 'handle-se', -30, 40)
      r = await rect(page)
      expect(Math.abs(r.w / r.h - ratio) / ratio, `${label} after se`).toBeLessThan(0.012)
      expect([r.x, r.y], `${label}: the opposite corner stayed`).toEqual([before.x, before.y])
      // an edge handle resizes both axes and keeps the shape
      await dragBy(page, 'handle-e', -25, 0)
      const e = await rect(page)
      expect(Math.abs(e.w / e.h - ratio) / ratio, `${label} after e`).toBeLessThan(0.012)
      expect(e.x).toBe(r.x)
      expect(e.w).toBeLessThan(r.w)
    }
  })

  test('numeric entry: snapping, clamping, and the line that says what changed', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await enter(page)
    const f = fields(page)
    await f.w.fill('503'); await f.w.blur()
    expect(await num(f.w)).toBe(500) // steps of 4, rounded down
    await expect(page.locator('#region-notes')).toContainText('Width snaps down to 500')
    await f.x.fill('101'); await f.x.blur()
    expect((await num(f.x)) % 2).toBe(0)
    await f.x.fill('5000'); await f.x.blur()
    expect(await num(f.x)).toBe(SW - 500)
    await expect(page.locator('#region-notes')).toContainText('X is limited to 0 to 780')
    await f.w.fill('5000'); await f.w.blur()
    expect(await rect(page)).toMatchObject({ x: 0, w: SW }) // the window slides back inside rather than refusing
    await expect(page.locator('#region-notes')).toContainText('Width is at most 1280')
    await f.h.fill('10'); await f.h.blur()
    expect(await num(f.h)).toBe(64)
    await expect(page.locator('#region-notes')).toContainText('Height is at least 64')
    await f.y.fill('-30'); await f.y.blur()
    expect(await num(f.y)).toBe(0)
    // the picture's overlay follows the numbers
    const label = await page.getByTestId('region-label').textContent()
    expect(label).toBe(`${SW} × 64 at 0, 0`)
  })

  test('numeric entry with a lock: width changes height and height changes width', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await enter(page)
    const f = fields(page)
    await setLock(page, '1:1')
    await f.w.fill('400'); await f.w.blur()
    expect(await rect(page)).toMatchObject({ w: 400, h: 400 })
    await setLock(page, '16:9')
    await f.w.fill('800'); await f.w.blur()
    expect(await rect(page)).toMatchObject({ w: 800, h: 450 })
    await f.h.fill('270'); await f.h.blur()
    expect(await rect(page)).toMatchObject({ w: 480, h: 270 })
    await setLock(page, 'Free')
    await f.h.fill('300'); await f.h.blur()
    expect(await rect(page)).toMatchObject({ w: 480, h: 300 })
    await setLock(page, 'Keep current shape')
    await f.w.fill('960'); await f.w.blur()
    expect(await rect(page)).toMatchObject({ w: 960, h: 600 })
  })

  test('Apply sends the region and the status and the picture follow; Full frame stays reachable', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await enter(page)
    const f = fields(page)
    for (const [k, v] of [['w', '403'], ['h', '301'], ['x', '101'], ['y', '51']] as const) { await f[k].fill(v); await f[k].blur() }
    const want = await rect(page)
    await page.getByRole('button', { name: 'Apply region' }).click()
    await expect(page.getByTestId('region-panel')).toHaveCount(0)
    await expect(page.getByTestId('region-done')).toContainText(`Region set: ${want.w}×${want.h} at ${want.x}, ${want.y}`)
    await expect.poll(async () => { const s = await status(page); return [s.roi.x, s.roi.y, s.roi.width, s.roi.height, s.camera.width, s.camera.height] }, { timeout: 10_000 }).toEqual([want.x, want.y, want.w, want.h, want.w, want.h])
    await expect(page.getByTestId('size-label')).toHaveText(`${want.w}×${want.h} at ${want.x}, ${want.y}`)
    // tracking restarted and the live picture is the region
    expect((await status(page)).tracking).toBeFalsy()
    // the way back to the whole sensor is on the confirmation line
    await page.getByTestId('region-done').getByRole('button', { name: 'Full frame' }).click()
    await expect.poll(async () => (await status(page)).camera.width, { timeout: 10_000 }).toBe(SW)
    await expect(page.getByTestId('size-label')).toHaveText('1280×960')
  })

  test('the API refuses a region outside the sensor or too small, with a reason; the presets still work', async ({ page }) => {
    for (const body of [{ x: 1000, y: 0, width: 400, height: 400 }, { x: 0, y: 0, width: 16, height: 16 }, { x: 0, y: 0, width: 5000, height: 400 }, { x: 0, y: 0 }]) {
      const r = await put(page, '/camera/roi', body)
      expect(r.status(), JSON.stringify(body)).toBe(400)
      expect((await r.json()).error).toMatch(/region|at least|outside|larger/)
    }
    const ok = await put(page, '/camera/roi', { x: 101, y: 51, width: 403, height: 301 })
    expect(await ok.json()).toEqual({ x: 102, y: 52, width: 400, height: 300 })
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 102, y: 52, width: 400, height: 300 })
    expect((await roiNow(page)).limits).toEqual({ step_x: 2, step_y: 2, step_w: 4, step_h: 2, min_w: 64, min_h: 64 })
    await put(page, '/camera/roi', { fraction: 0.5 })
    await expect.poll(async () => (await status(page)).camera.width).toBe(640)
  })

  test('Cancel and Escape put the earlier region back; Full frame in the panel applies the whole sensor', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await put(page, '/camera/roi', { x: 100, y: 60, width: 400, height: 300 })
    await expect.poll(async () => (await status(page)).camera.width).toBe(400)
    await enter(page)
    expect(await rect(page)).toEqual({ x: 100, y: 60, w: 400, h: 300 })
    await expect.poll(async () => (await status(page)).camera.width).toBe(SW)
    await dragBy(page, 'region-body', 50, 50)
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByTestId('region-panel')).toHaveCount(0)
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 100, y: 60, width: 400, height: 300 })
    await expect.poll(async () => (await status(page)).camera.width).toBe(400)
    // Escape on the selection does the same
    await enter(page)
    await page.getByTestId('region-select').focus()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('region-panel')).toHaveCount(0)
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 100, y: 60, width: 400, height: 300 })
    // Full frame
    await enter(page)
    await page.getByRole('button', { name: 'Full frame' }).click()
    expect(await rect(page)).toEqual({ x: 0, y: 0, w: SW, h: SH })
    await expect(page.getByTestId('region-result')).toContainText('the whole sensor')
    await page.getByRole('button', { name: 'Apply region' }).click()
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 0, y: 0, width: SW, height: SH })
  })

  test('keyboard: arrows move by the step, shift+arrows resize, Enter applies', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await enter(page)
    await page.getByTestId('region-select').focus()
    await page.keyboard.press('ArrowRight'); await page.keyboard.press('ArrowDown')
    expect(await rect(page)).toEqual({ x: 322, y: 242, w: 640, h: 480 })
    await page.keyboard.press('Shift+ArrowRight'); await page.keyboard.press('Shift+ArrowDown'); await page.keyboard.press('Shift+ArrowDown')
    expect(await rect(page)).toEqual({ x: 322, y: 242, w: 644, h: 484 })
    await page.keyboard.press('Shift+ArrowLeft')
    expect((await rect(page)).w).toBe(640)
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('region-panel')).toHaveCount(0)
    await expect.poll(async () => (await roiNow(page))).toMatchObject({ x: 320, y: 242, width: 640, height: 484 })
  })

  test('greyed with its reason while recording, and while the camera has no region support', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await post(page, '/capture/sequence', { frames: 600, format: 'ser' })
    await expect.poll(async () => (await status(page)).capture.state).not.toBe('idle')
    try {
    await page.goto('/')
    await page.getByRole('tab', { name: /^Camera/ }).click()
    await page.getByRole('button', { name: 'Picture tools' }).click()
    const btn = page.getByRole('button', { name: 'Region…' })
    await expect(btn).toBeDisabled()
    const why = await btn.getAttribute('aria-describedby')
    expect(why).toBeTruthy()
    await expect(page.locator(`#${why}`)).toContainText('not while recording')
    await post(page, '/capture/stop')
    await expect.poll(async () => (await status(page)).capture.state, { timeout: 30_000 }).toBe('idle') // a 600-frame SER takes a while to flush on a busy runner
    await expect(btn).toBeEnabled()
    } finally {
      await post(page, '/capture/stop') // never leave a recording running for the next test
      await expect.poll(async () => (await status(page)).capture.state, { timeout: 30_000 }).toBe('idle')
    }
  })

  test('greyed with its reason when the camera cannot read a region', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.routeWebSocket(/\/api\/ws/, (ws) => {
      const server = ws.connectToServer()
      server.onMessage((m) => { try { const s = JSON.parse(String(m)); s.roi = null; ws.send(JSON.stringify(s)) } catch { ws.send(m) } })
      ws.onMessage((m) => server.send(m))
    })
    await page.goto('/')
    await page.getByRole('button', { name: 'Picture tools' }).click()
    const btn = page.getByRole('button', { name: 'Region…' })
    await expect(btn).toBeDisabled()
    await expect(page.locator(`#${await btn.getAttribute('aria-describedby')}`)).toContainText('cannot read out part of its sensor')
  })
})

test.describe('layout', () => {
  const SIZES = [{ w: 390, h: 844 }, { w: 375, h: 667 }, { w: 667, h: 375 }, { w: 1280, h: 800 }]
  for (const theme of ['night', 'day'] as const) {
    for (const { w, h } of SIZES) {
      test(`${w}x${h}, ${theme} theme, wide font: the panel and the picture do not overlap and nothing is clipped`, async ({ page }) => {
        await put(page, '/ui', { key: 'ui.theme', value: theme })
        await page.setViewportSize({ width: w, height: h })
        await page.addInitScript((css) => { document.addEventListener('DOMContentLoaded', () => { const s = document.createElement('style'); s.textContent = css; document.head.appendChild(s) }) }, WIDE)
        await enter(page)
        const view = page.getByTestId('region-select')
        const vb = (await view.boundingBox())!
        const panel = page.getByTestId('region-panel')
        const pb = (await panel.boundingBox())!
        const overlap = Math.max(0, Math.min(vb.x + vb.width, pb.x + pb.width) - Math.max(vb.x, pb.x)) * Math.max(0, Math.min(vb.y + vb.height, pb.y + pb.height) - Math.max(vb.y, pb.y))
        expect(overlap, 'picture and panel overlap').toBe(0)
        // the picture is on screen whole
        expect(vb.x).toBeGreaterThanOrEqual(-0.5); expect(vb.y).toBeGreaterThanOrEqual(-0.5)
        expect(vb.x + vb.width).toBeLessThanOrEqual(w + 0.5); expect(vb.y + vb.height).toBeLessThanOrEqual(h + 0.5)
        // every field, the lock and the buttons are inside the panel's width (no horizontal clipping), and reachable by scrolling
        const main = page.locator('main.panel')
        const sw = await main.evaluate((el) => [el.scrollWidth, el.clientWidth])
        expect(sw[0]).toBeLessThanOrEqual(sw[1] + 1)
        for (const t of [page.locator('#region-x'), page.locator('#region-y'), page.locator('#region-w'), page.locator('#region-h'), page.getByLabel('Aspect lock'), page.getByRole('button', { name: 'Centre' }), page.getByRole('button', { name: 'Full frame' }), page.getByRole('button', { name: 'Cancel' }), page.getByRole('button', { name: 'Apply region' })]) {
          await t.scrollIntoViewIfNeeded()
          const b = (await t.boundingBox())!
          const m = (await main.boundingBox())!
          expect(b.x, 'left edge').toBeGreaterThanOrEqual(m.x - 0.5); expect(b.x + b.width, 'right edge').toBeLessThanOrEqual(m.x + m.width + 0.5)
          expect(b.height, 'a 44 px target').toBeGreaterThanOrEqual(43.5)
        }
        // Apply is in reach without scrolling the page: sticky at the bottom of the panel, above the tab bar
        const apply = (await page.getByRole('button', { name: 'Apply region' }).boundingBox())!
        const tabs = (await page.getByRole('tablist').boundingBox())!
        if (h > w) expect(apply.y + apply.height).toBeLessThanOrEqual(tabs.y + 0.5)
        expect(apply.y + apply.height).toBeLessThanOrEqual(h + 0.5)
        // handles are inside the picture and on screen, and none is clipped
        for (const hnd of ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']) {
          const b = (await page.getByTestId(`handle-${hnd}`).boundingBox())!
          expect(b.x).toBeGreaterThanOrEqual(vb.x - 0.5); expect(b.y).toBeGreaterThanOrEqual(vb.y - 0.5)
          expect(b.x + b.width).toBeLessThanOrEqual(vb.x + vb.width + 0.5); expect(b.y + b.height).toBeLessThanOrEqual(vb.y + vb.height + 0.5)
        }
        // the whole-sensor choice also puts the handles at the edges: every one stays on the picture
        await page.getByRole('button', { name: 'Full frame' }).click()
        for (const hnd of ['nw', 'se', 'n', 'w']) {
          const b = (await page.getByTestId(`handle-${hnd}`).boundingBox())!
          expect(b.x).toBeGreaterThanOrEqual(vb.x - 0.5); expect(b.x + b.width).toBeLessThanOrEqual(vb.x + vb.width + 0.5)
        }
        await page.getByRole('button', { name: 'Cancel' }).click()
        await expect(page.getByTestId('region-panel')).toHaveCount(0)
      })
    }
  }

  test('the picture takes no page scroll or zoom gestures while choosing (touch-action none)', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await enter(page)
    for (const id of ['region-select', 'region-body', 'handle-se']) expect(await page.getByTestId(id).evaluate((el) => getComputedStyle(el).touchAction)).toBe('none')
    // the wheel does not zoom the picture while choosing
    const b = await pic(page)
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
    await page.mouse.wheel(0, -400)
    await expect(page.locator('.zoomed')).toHaveCSS('transform', /matrix\(1, 0, 0, 1, 0, 0\)|none/)
  })
})
