import { expect, test } from '@playwright/test'
import { centreRect, dataNote, fullRect, halfRect, isCentred, keyRect, lockedSize, lockRatio, Limits, moveRect, Rect, regionLabel, reshape, resizeRect, Sensor, setField, snapRect } from '../src/region'

// The geometry of choosing a region, called directly: no page, no server. (Playwright's runner is the one test runner the project has.)
const ASI: Limits = { step_x: 2, step_y: 2, step_w: 8, step_h: 2, min_w: 64, min_h: 64 }
const POA: Limits = { step_x: 2, step_y: 2, step_w: 4, step_h: 2, min_w: 64, min_h: 64 }
const ONE: Limits = { step_x: 1, step_y: 1, step_w: 1, step_h: 1, min_w: 16, min_h: 16 }
const S: Sensor = { w: 3008, h: 3008 }
const SIM: Sensor = { w: 1280, h: 960 }
const r = (x: number, y: number, w: number, h: number): Rect => ({ x, y, w, h })
const inside = (q: Rect, s: Sensor) => q.x >= 0 && q.y >= 0 && q.x + q.w <= s.w && q.y + q.h <= s.h

test.beforeEach(async ({}, info) => { test.skip(info.project.name !== 'phone', 'pure functions: once is enough') })

test('snapping: size down to the steps, corner to the nearest step, then inside the sensor', () => {
  expect(snapRect(r(13, 7, 1503, 1501), ASI, S)).toEqual(r(14, 8, 1496, 1500))
  expect(snapRect(r(13, 7, 1503, 1501), POA, S)).toEqual(r(14, 8, 1500, 1500))
  expect(snapRect(r(13, 7, 1503, 1501), ONE, S)).toEqual(r(13, 7, 1503, 1501))
  // the corner rounds to nearest, so 3 goes to 4 and 5 to 6 on a step of 2... (Math.round: 1.5 -> 2)
  expect(snapRect(r(3, 5, 100, 100), ASI, S)).toEqual(r(4, 6, 96, 100))
})

test('clamping at all four edges and at the minimum', () => {
  expect(snapRect(r(-50, -50, 400, 400), ASI, S)).toEqual(r(0, 0, 400, 400))
  expect(snapRect(r(2900, 100, 400, 400), ASI, S)).toEqual(r(2608, 100, 400, 400))
  expect(snapRect(r(100, 2900, 400, 400), ASI, S)).toEqual(r(100, 2608, 400, 400))
  expect(snapRect(r(2900, 2900, 400, 400), ASI, S)).toEqual(r(2608, 2608, 400, 400))
  expect(snapRect(r(0, 0, 10, 10), ASI, S)).toEqual(r(0, 0, 64, 64))
  expect(snapRect(r(0, 0, 99999, 99999), ASI, S)).toEqual(r(0, 0, 3008, 3008))
  for (const q of [r(2999, 2999, 9, 9), r(-3, 5, 70, 70), r(1, 1, 3008, 3008)]) expect(inside(snapRect(q, ASI, S), S)).toBe(true)
  // an odd sensor: the largest window is a whole number of steps
  expect(snapRect(r(0, 0, 1281, 961), ASI, { w: 1281, h: 961 })).toEqual(r(0, 0, 1280, 960))
})

test('moving keeps the size and stops at the edges', () => {
  const start = r(752, 752, 1504, 1504)
  expect(moveRect(start, 100, -50, ASI, S)).toEqual(r(852, 702, 1504, 1504))
  expect(moveRect(start, 5000, 5000, ASI, S)).toEqual(r(1504, 1504, 1504, 1504))
  expect(moveRect(start, -5000, -5000, ASI, S)).toEqual(r(0, 0, 1504, 1504))
  expect(moveRect(start, 0.9, 0.9, ASI, S)).toEqual(r(752, 752, 1504, 1504)) // under half a step: the corner stays on its step
})

test('free resize: the opposite side stays where it was', () => {
  const start = r(1000, 1000, 800, 600)
  const se = resizeRect(start, 'se', 100, 40, null, ASI, S)
  expect(se).toEqual(r(1000, 1000, 896, 640))
  const nw = resizeRect(start, 'nw', -100, -40, null, ASI, S)
  expect([nw.x + nw.w, nw.y + nw.h]).toEqual([1800, 1600])
  expect([nw.w, nw.h]).toEqual([896, 640])
  const e = resizeRect(start, 'e', 120, 999, null, ASI, S) // an edge handle never changes the other axis
  expect([e.y, e.h, e.w]).toEqual([1000, 600, 920])
  const n = resizeRect(start, 'n', 999, -60, null, ASI, S)
  expect([n.x, n.w, n.h, n.y + n.h]).toEqual([1000, 800, 660, 1600])
})

test('free resize: not below the minimum, not past the sensor, and dragging across the opposite side stops at the minimum', () => {
  const start = r(1000, 1000, 800, 600)
  expect(resizeRect(start, 'se', -5000, -5000, null, ASI, S)).toEqual(r(1000, 1000, 64, 64))
  expect(resizeRect(start, 'nw', 5000, 5000, null, ASI, S)).toEqual(r(1736, 1536, 64, 64))
  expect(resizeRect(start, 'se', 5000, 5000, null, ASI, S)).toEqual(r(1000, 1000, 2008, 2008))
  expect(resizeRect(start, 'nw', -5000, -5000, null, ASI, S)).toEqual(r(0, 0, 1800, 1600))
  expect(resizeRect(start, 'w', -5000, 0, null, ASI, S)).toEqual(r(0, 1000, 1800, 600))
})

test('locked shape: a corner keeps its opposite corner and the shape for every lock', () => {
  const start = r(1000, 1000, 800, 600)
  for (const id of ['1:1', '4:3', '16:9', '3:2', '2:1', 'sensor', 'current'] as const) {
    const ratio = lockRatio(id, start, S)!
    for (const h of ['nw', 'ne', 'se', 'sw'] as const) {
      const q = resizeRect(reshape(start, ratio, ASI, S), h, 90, 70, ratio, ASI, S)
      expect(inside(q, S), `${id} ${h} inside`).toBe(true)
      expect(Math.abs(q.w / q.h - ratio) / ratio, `${id} ${h} shape`).toBeLessThan(0.01)
      expect(q.w % 8 + q.h % 2 + q.x % 2 + q.y % 2, `${id} ${h} on the steps`).toBe(0)
    }
  }
  // the opposite corner is fixed
  const sq = reshape(start, 1, ASI, S)
  const se = resizeRect(sq, 'se', 120, 20, 1, ASI, S)
  expect([se.x, se.y]).toEqual([sq.x, sq.y])
  const nw = resizeRect(sq, 'nw', -120, -20, 1, ASI, S)
  expect([nw.x + nw.w, nw.y + nw.h]).toEqual([sq.x + sq.w, sq.y + sq.h])
  expect(nw.w).toBe(nw.h)
})

test('locked shape: 16:9, 1:1 and the sensor shape give the exact numbers on a 3008 square sensor', () => {
  expect(lockedSize(1504, 1, ASI, S)).toEqual({ w: 1504, h: 1504 })
  expect(lockedSize(1600, 16 / 9, ASI, S)).toEqual({ w: 1600, h: 900 })
  expect(lockedSize(4000, 16 / 9, ASI, S)).toEqual({ w: 3008, h: 1692 })
  expect(lockedSize(10, 4 / 3, ASI, S)).toEqual({ w: 88, h: 66 }) // at least 64 high, so at least 85.3 wide, on the steps of 8
  expect(lockedSize(1280, lockRatio('sensor', r(0, 0, 100, 100), SIM)!, POA, SIM)).toEqual({ w: 1280, h: 960 })
})

test('locked shape: an edge handle resizes both axes, the opposite edge stays and the other axis is centred', () => {
  const sq = r(1000, 1000, 800, 800)
  const e = resizeRect(sq, 'e', 200, 0, 1, ASI, S)
  expect([e.x, e.w, e.h]).toEqual([1000, 1000, 1000])
  expect(e.y + e.h / 2).toBe(1400)
  const w = resizeRect(sq, 'w', -200, 0, 1, ASI, S)
  expect([w.x + w.w, w.w, w.h]).toEqual([1800, 1000, 1000])
  const n = resizeRect(sq, 'n', 0, -200, 1, ASI, S)
  expect([n.y + n.h, n.w, n.h]).toEqual([1800, 1000, 1000])
  expect(n.x + n.w / 2).toBe(1400)
  const s = resizeRect(sq, 's', 0, 200, 1, ASI, S)
  expect([s.y, s.h, s.w]).toEqual([1000, 1000, 1000])
  // at the sensor edge it stops instead of leaving
  const big = resizeRect(sq, 'e', 9999, 0, 1, ASI, S)
  expect(inside(big, S)).toBe(true)
  expect(big.w).toBe(2008)
})

test('numeric entry without a lock: snapping, clamping and the words that say so', () => {
  const cur = r(100, 100, 800, 600)
  let q = setField(cur, 'w', 1003, null, ASI, S)
  expect(q.rect.w).toBe(1000); expect(q.note).toMatch(/snaps down to 1000/)
  q = setField(cur, 'w', 20, null, ASI, S)
  expect(q.rect.w).toBe(64); expect(q.note).toMatch(/at least 64/)
  q = setField(cur, 'h', 99999, null, ASI, S)
  expect(q.rect.h).toBe(3008); expect(q.note).toMatch(/at most 3008/)
  q = setField(cur, 'x', 2500, null, ASI, S)
  expect(q.rect.x).toBe(2208); expect(q.note).toMatch(/limited to 0 to 2208/)
  q = setField(cur, 'y', -4, null, ASI, S)
  expect(q.rect.y).toBe(0); expect(q.note).toMatch(/limited to 0 to/)
  q = setField(cur, 'x', 101, null, ASI, S)
  expect(q.rect.x % 2).toBe(0); expect(q.note).toMatch(/snaps to/)
  q = setField(cur, 'x', 120, null, ASI, S)
  expect(q.note).toBeNull()
  // a width that no longer fits at this offset slides the window back inside instead of refusing
  q = setField(r(2000, 100, 800, 600), 'w', 2000, null, ASI, S)
  expect(q.rect).toEqual(r(1008, 100, 2000, 600))
  expect(setField(cur, 'w', NaN, null, ASI, S).note).toMatch(/whole number/)
})

test('numeric entry with a lock: width sets height and height sets width', () => {
  const cur = r(100, 100, 1600, 900)
  const ratio = 16 / 9
  let q = setField(cur, 'w', 800, ratio, ASI, S)
  expect([q.rect.w, q.rect.h]).toEqual([800, 450]); expect(q.note).toBeNull()
  q = setField(cur, 'h', 450, ratio, ASI, S)
  expect([q.rect.w, q.rect.h]).toEqual([800, 450])
  q = setField(cur, 'h', 451, ratio, ASI, S)
  expect(q.rect.h % 2).toBe(0); expect(q.note).toMatch(/became/)
  q = setField(cur, 'w', 99999, ratio, ASI, S)
  expect([q.rect.w, q.rect.h]).toEqual([3008, 1692])
  expect(inside(q.rect, S)).toBe(true)
  q = setField(cur, 'w', 10, 1, ASI, S)
  expect([q.rect.w, q.rect.h]).toEqual([64, 64])
  // x and y are not tied to the shape
  q = setField(cur, 'x', 500, ratio, ASI, S)
  expect([q.rect.x, q.rect.w, q.rect.h]).toEqual([500, 1600, 900])
})

test('choosing a lock reshapes about the centre; Centre, Full frame and the half-sensor start', () => {
  const q = reshape(r(500, 800, 1000, 500), 1, ASI, S)
  expect(q.w).toBe(q.h)
  expect(inside(q, S)).toBe(true)
  expect(Math.abs((q.x + q.w / 2) - 1000)).toBeLessThanOrEqual(4)
  expect(centreRect(r(0, 0, 1504, 1504), ASI, S)).toEqual(r(752, 752, 1504, 1504))
  expect(fullRect(ASI, S)).toEqual(r(0, 0, 3008, 3008))
  expect(halfRect(ASI, S)).toEqual(r(752, 752, 1504, 1504))
  expect(halfRect(POA, SIM)).toEqual(r(320, 240, 640, 480))
  expect(lockRatio('free', q, S)).toBeNull()
  expect(lockRatio('current', r(0, 0, 400, 100), S)).toBe(4)
  expect(lockRatio('sensor', q, SIM)).toBeCloseTo(4 / 3)
})

test('arrow keys move by the step, shift+arrows resize by the step, the shape holds when locked', () => {
  const cur = r(1000, 1000, 800, 600)
  expect(keyRect(cur, 'ArrowRight', false, null, ASI, S)).toEqual(r(1002, 1000, 800, 600))
  expect(keyRect(cur, 'ArrowUp', false, null, ASI, S)).toEqual(r(1000, 998, 800, 600))
  expect(keyRect(cur, 'ArrowRight', true, null, ASI, S)).toEqual(r(1000, 1000, 808, 600))
  expect(keyRect(cur, 'ArrowDown', true, null, ASI, S)).toEqual(r(1000, 1000, 800, 602))
  expect(keyRect(cur, 'ArrowLeft', true, null, ASI, S)).toEqual(r(1000, 1000, 792, 600))
  expect(keyRect(cur, 'a', false, null, ASI, S)).toBeNull()
  expect(keyRect(r(0, 0, 800, 600), 'ArrowLeft', false, null, ASI, S)).toEqual(r(0, 0, 800, 600))
  const sq = r(1000, 1000, 800, 800)
  const g = keyRect(sq, 'ArrowRight', true, 1, ASI, S)!
  expect(g.w).toBe(g.h)
  expect(g.w).toBe(808)
  expect(keyRect(cur, 'ArrowRight', false, null, ASI, S, true)).toEqual(r(1020, 1000, 800, 600))
})

test('the words: size label with the offset only when off centre, and the data saving', () => {
  expect(regionLabel(r(752, 752, 1504, 1504), S, ASI)).toBe('1504×1504')
  expect(regionLabel(r(100, 752, 1504, 1504), S, ASI)).toBe('1504×1504 at 100, 752')
  expect(regionLabel(r(0, 0, 3008, 3008), S, ASI)).toBe('3008×3008')
  expect(isCentred(r(752, 752, 1504, 1504), S, ASI)).toBe(true)
  expect(dataNote(r(752, 752, 1504, 1504), S)).toBe('about 4× less data per frame')
  expect(dataNote(r(0, 0, 3008, 3008), S)).toMatch(/no saving/)
  expect(dataNote(r(0, 0, 640, 480), SIM)).toBe('about 4× less data per frame')
  expect(dataNote(r(0, 0, 200, 200), S)).toBe('about 226× less data per frame')
})
