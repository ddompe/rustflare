import { expect, Page } from '@playwright/test'

/**
 * One known starting state for every browser test. The tests share one server and one data folder, so a test that fails halfway must not
 * leave a recording, a profile or a region behind for the next one. Call it from `beforeEach`, and `stopCapture` from `afterEach`.
 */
export async function stopCapture(page: Page) {
  await page.request.post('/api/capture/stop', { data: {} })
  // a long SER takes a while to flush on a busy runner: wait for it rather than starting the next test on a busy recorder
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).capture.state, { timeout: 30_000 }).toBe('idle')
}

export async function resetSimulators(page: Page) {
  const post = (path: string, data: unknown = {}) => page.request.post(`/api${path}`, { data })
  const put = (path: string, data: unknown) => page.request.put(`/api${path}`, { data })
  await stopCapture(page)
  // a calibration, guiding or a tracker left running by an earlier (possibly failed) test disturbs every test after it
  await post('/guiding/stop'); await post('/tracking/stop')
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).guiding?.state, { timeout: 15_000 }).not.toBe('calibrating')
  await put('/storage/reserve', { gb: 2 }) // the test server records to its own disk: keep its reserve small (a CI runner has little free)
  await post('/camera/close'); await post('/mount/disconnect')
  await post('/camera/open', { driver: 'simulator', id: 'sim0' })
  await post('/mount/connect', { driver: 'simulator' })
  await put('/profile', { id: 'sun_ha' })
  await expect.poll(async () => (await (await page.request.get('/api/status')).json()).camera?.fps ?? 0, { timeout: 10_000 }).toBeGreaterThan(1)
}
