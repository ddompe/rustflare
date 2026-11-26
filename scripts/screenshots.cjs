#!/usr/bin/env node
/**
 * Takes the README / manual screenshots of a running RustFlare, with the same states every time.
 *
 *   node scripts/screenshots.cjs <base-url> <out-dir> [--only name,name] [--no-drive]
 *
 * It only looks and taps tabs and disclosures: it never starts a recording, moves the mount or changes a setting. Get the state you
 * want first (camera open, mount connected, Sun in view and tracked, calibrated, holding; a few finished stacks), then run it.
 * Works on the simulators too, which is how to try it. Needs Playwright: run it from web/ (`cd web && node ../scripts/screenshots.cjs ...`)
 * with PLAYWRIGHT_BROWSERS_PATH set as for the e2e tests. Output: PNG at 3x for phones; convert with `cwebp -q 88` before committing.
 * Privacy: it never opens More > Site or the device cards' "This unit" rows (site coordinates, serial numbers) and blurs any text that
 * matches a coordinate, a serial or an address, as a second guard. Look at every picture before it goes into git.
 */
const path = require('path')
const { chromium, devices } = require(path.join(process.cwd(), 'node_modules/@playwright/test'))

const [base, out] = process.argv.slice(2)
if (!base || !out) { console.error('usage: screenshots.cjs <base-url> <out-dir> [--only a,b]'); process.exit(2) }
const only = (process.argv.find((a) => a.startsWith('--only=')) || '').slice(7).split(',').filter(Boolean)
const want = (n) => only.length === 0 || only.includes(n)
const fs = require('fs'); fs.mkdirSync(out, { recursive: true })

// anything that looks like a coordinate pair, a serial, an IP/MAC or a host name is blurred in the picture
const GUARD = `
  const re = /(-?\\d{1,3}\\.\\d{3,}\\s*[°,]?\\s*,?\\s*-?\\d{1,3}\\.\\d{3,})|(\\b[A-Z0-9]{12,}\\b)|(\\b\\d{1,3}(\\.\\d{1,3}){3}\\b)|(\\b([0-9A-F]{2}:){5}[0-9A-F]{2}\\b)|(\\b[\\w-]+\\.local\\b)/i
  const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT); const hit = []
  while (w.nextNode()) { if (re.test(w.currentNode.textContent)) hit.push(w.currentNode.parentElement) }
  hit.forEach((e) => { if (e) e.style.filter = 'blur(6px)' }); hit.length`

async function guard(page, name) {
  const n = await page.evaluate(GUARD)
  if (n) console.log(`  ${name}: blurred ${n} text item(s) that looked private: check the picture`)
}

;(async () => {
  const browser = await chromium.launch()
  const phone = { ...devices['iPhone 14 Pro'], deviceScaleFactor: 3 }
  const shoot = async (ctxOpts, name, f) => {
    if (!want(name)) return
    const ctx = await browser.newContext({ ...ctxOpts, colorScheme: 'dark' }); const page = await ctx.newPage()
    try {
      await page.goto(base, { waitUntil: 'networkidle' }); await page.waitForTimeout(1500)
      await f(page); await page.waitForTimeout(500); await guard(page, name)
      await page.screenshot({ path: path.join(out, `${name}.png`) }); console.log('ok', name)
    } catch (e) { console.error('FAILED', name, e.message.split('\n')[0]) }
    await ctx.close()
  }
  const tab = (n) => (p) => p.getByRole('tab', { name: new RegExp('^' + n) }).click().then(() => p.waitForTimeout(800))

  await shoot(phone, 'phone-camera', tab('Camera'))
  await shoot(phone, 'phone-aim', async (p) => { await tab('Aim')(p); await p.locator('.offset, .home').first().scrollIntoViewIfNeeded().catch(() => {}) })
  await shoot(phone, 'phone-track', tab('Track'))
  await shoot(phone, 'phone-record', tab('Record'))
  await shoot(phone, 'phone-after-burst', async (p) => { await tab('Record')(p); await p.getByText('After each burst', { exact: true }).click(); await p.getByText('After each burst', { exact: true }).scrollIntoViewIfNeeded() })
  await shoot(phone, 'phone-files-stacks', async (p) => { await p.getByRole('button', { name: /^Files/ }).click(); await p.waitForTimeout(800); const s = p.locator('.fm-crumb, .fm-tree button', { hasText: 'stacks' }).first(); if (await s.count()) await s.click(); await p.waitForTimeout(1500) })
  await shoot(phone, 'phone-hide-controls', async (p) => { await p.getByRole('button', { name: 'Hide controls' }).click() })
  await shoot({ ...devices['iPhone 14 Pro landscape'], deviceScaleFactor: 3 }, 'phone-landscape', async () => {})
  await shoot({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 }, 'desktop-live', async (p) => { await tab('Camera')(p) })
  await shoot({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 }, 'desktop-files', async (p) => { await p.getByRole('button', { name: /^Files/ }).click(); await p.waitForTimeout(1200) })
  await browser.close()
})()
