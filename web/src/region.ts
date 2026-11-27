/**
 * The geometry of choosing a region of interest, in sensor pixels. Pure functions, no DOM: the selection overlay and the numeric fields both
 * call these, so a dragged corner and a typed number land on the same grid, and the grid is the one the engine rounds to
 * (`fit_roi` in rustflare-core): the size rounds DOWN to the steps, the corner to the NEAREST step, and the window stays inside the sensor.
 */

export type Rect = { x: number; y: number; w: number; h: number }
export type Limits = { step_x: number; step_y: number; step_w: number; step_h: number; min_w: number; min_h: number }
export type Sensor = { w: number; h: number }
export type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw'
export const HANDLES: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

export type LockId = 'free' | 'current' | 'sensor' | '1:1' | '4:3' | '3:2' | '16:9' | '2:1'
export const LOCKS: { id: LockId; label: string }[] = [
  { id: 'free', label: 'Free' },
  { id: 'current', label: 'Keep current shape' },
  { id: 'sensor', label: 'Sensor shape' },
  { id: '1:1', label: '1:1' },
  { id: '4:3', label: '4:3' },
  { id: '3:2', label: '3:2' },
  { id: '16:9', label: '16:9' },
  { id: '2:1', label: '2:1' },
]

const st = (v: number) => Math.max(1, Math.round(v))
const down = (v: number, s: number) => Math.floor(v / st(s)) * st(s)
const up = (v: number, s: number) => Math.ceil(v / st(s)) * st(s)
const nearest = (v: number, s: number) => Math.round(v / st(s)) * st(s)
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))

/** The smallest width and height the camera takes (never below one step). */
export const minSize = (l: Limits) => ({ w: Math.max(l.min_w, st(l.step_w)), h: Math.max(l.min_h, st(l.step_h)) })
/** The largest width and height on this sensor, on the steps. */
export const maxSize = (l: Limits, s: Sensor) => ({ w: down(s.w, l.step_w), h: down(s.h, l.step_h) })

/** Width over height for a lock, or null for Free. `current` is the shape of `cur`. */
export function lockRatio(id: LockId, cur: Rect, s: Sensor): number | null {
  switch (id) {
    case 'free': return null
    case 'current': return cur.w / cur.h
    case 'sensor': return s.w / s.h
    default: { const [a, b] = id.split(':').map(Number); return a / b }
  }
}

/** Puts a rectangle on the camera's grid and inside the sensor: size down to the steps (not below the minimum, not above the sensor), corner to the nearest step, then moved back inside. */
export function snapRect(r: Rect, l: Limits, s: Sensor): Rect {
  const mn = minSize(l), mx = maxSize(l, s)
  const w = clamp(down(r.w, l.step_w), Math.min(mn.w, mx.w), mx.w)
  const h = clamp(down(r.h, l.step_h), Math.min(mn.h, mx.h), mx.h)
  const x = clamp(nearest(r.x, l.step_x), 0, Math.max(0, down(s.w - w, l.step_x)))
  const y = clamp(nearest(r.y, l.step_y), 0, Math.max(0, down(s.h - h, l.step_y)))
  return { x, y, w, h }
}

/**
 * The largest size not above `w` wide that has shape `ratio` (width over height) as closely as the steps allow, within the minimum and the sensor.
 * Returns whole steps, so the shape is exact only when the steps allow it (16:9 on a 2-pixel row step is within one row).
 */
export function lockedSize(w: number, ratio: number, l: Limits, s: Sensor): { w: number; h: number } {
  const mn = minSize(l), mx = maxSize(l, s)
  const wLo = Math.max(mn.w, up(mn.h * ratio, l.step_w))
  const wHi = Math.min(mx.w, down(mx.h * ratio, l.step_w))
  let ww = down(clamp(w, Math.min(wLo, wHi), wHi), l.step_w)
  let hh = clamp(down(Math.round(ww / ratio), l.step_h), mn.h, mx.h)
  // rounding the height down can leave it a hair above the sensor after the width was lifted to the minimum
  while (hh > mx.h && ww > mn.w) { ww -= st(l.step_w); hh = clamp(down(Math.round(ww / ratio), l.step_h), mn.h, mx.h) }
  return { w: ww, h: hh }
}

/** The same shape and centre, reshaped to `ratio` (keeps the width where the sensor allows it). Used when a lock is chosen. */
export function reshape(r: Rect, ratio: number, l: Limits, s: Sensor): Rect {
  const { w, h } = lockedSize(r.w, ratio, l, s)
  return snapRect({ x: r.x + (r.w - w) / 2, y: r.y + (r.h - h) / 2, w, h }, l, s)
}

/** Moves the rectangle by (dx, dy) sensor pixels from where the drag began. The size never changes. */
export function moveRect(start: Rect, dx: number, dy: number, l: Limits, s: Sensor): Rect {
  return snapRect({ ...start, x: start.x + dx, y: start.y + dy }, l, s)
}

const has = (h: Handle, c: 'n' | 's' | 'e' | 'w') => h.includes(c)

/**
 * Drags `handle` by (dx, dy) sensor pixels from the rectangle `start`.
 * Free: each side moves on its own and the opposite side stays put; the window cannot be made smaller than the minimum or leave the sensor.
 * Locked (`ratio` given): a corner keeps its opposite corner and the shape; an edge handle keeps the opposite edge and resizes the other axis
 * about the centre line, so the picture grows or shrinks around the middle of the edge you hold.
 */
export function resizeRect(start: Rect, handle: Handle, dx: number, dy: number, ratio: number | null, l: Limits, s: Sensor): Rect {
  const mn = minSize(l)
  const left = start.x, right = start.x + start.w, top = start.y, bottom = start.y + start.h
  // the fixed side on each axis, and the raw position of the moving side
  const fx = has(handle, 'w') ? right : has(handle, 'e') ? left : null
  const fy = has(handle, 'n') ? bottom : has(handle, 's') ? top : null
  const mxPos = has(handle, 'w') ? left + dx : has(handle, 'e') ? right + dx : null
  const myPos = has(handle, 'n') ? top + dy : has(handle, 's') ? bottom + dy : null

  if (ratio == null) {
    let x = start.x, w = start.w, y = start.y, h = start.h
    if (fx != null && mxPos != null) {
      const east = has(handle, 'e')
      const room = east ? s.w - fx : fx // pixels between the fixed side and the sensor edge
      const raw = east ? mxPos - fx : fx - mxPos
      w = clamp(down(raw, l.step_w), Math.min(mn.w, down(room, l.step_w)), Math.max(mn.w, down(room, l.step_w)))
      x = east ? fx : fx - w
    }
    if (fy != null && myPos != null) {
      const south = has(handle, 's')
      const room = south ? s.h - fy : fy
      const raw = south ? myPos - fy : fy - myPos
      h = clamp(down(raw, l.step_h), Math.min(mn.h, down(room, l.step_h)), Math.max(mn.h, down(room, l.step_h)))
      y = south ? fy : fy - h
    }
    return snapRect({ x, y, w, h }, l, s)
  }

  // locked shape
  const corner = fx != null && fy != null
  let rawW: number
  if (corner) {
    const east = has(handle, 'e'), south = has(handle, 's')
    const ww = east ? mxPos! - fx! : fx! - mxPos!
    const hh = south ? myPos! - fy! : fy! - myPos!
    rawW = Math.max(ww, hh * ratio) // the rectangle always reaches the pointer on the axis that asks for more
    const roomW = east ? s.w - fx! : fx!, roomH = south ? s.h - fy! : fy!
    const sz = lockedSize(Math.min(rawW, roomW, roomH * ratio), ratio, l, s)
    const x = east ? fx! : fx! - sz.w, y = south ? fy! : fy! - sz.h
    return snapRect({ x, y, w: sz.w, h: sz.h }, l, s)
  }
  if (fx != null) { // an east or west edge: width from the pointer, height follows about the centre line
    const east = has(handle, 'e')
    rawW = east ? mxPos! - fx : fx - mxPos!
    const room = east ? s.w - fx : fx
    const sz = lockedSize(Math.min(rawW, room), ratio, l, s)
    const cy = start.y + start.h / 2
    return snapRect({ x: east ? fx : fx - sz.w, y: cy - sz.h / 2, w: sz.w, h: sz.h }, l, s)
  }
  // a north or south edge: height from the pointer, width follows about the centre line
  const south = has(handle, 's')
  const rawH = south ? myPos! - fy! : fy! - myPos!
  const room = south ? s.h - fy! : fy!
  const sz = lockedSize(Math.min(rawH, room) * ratio, ratio, l, s)
  const cx = start.x + start.w / 2
  return snapRect({ x: cx - sz.w / 2, y: south ? fy! : fy! - sz.h, w: sz.w, h: sz.h }, l, s)
}

export type Field = 'x' | 'y' | 'w' | 'h'
export type FieldResult = { rect: Rect; note: string | null }

/**
 * A number typed into one of the four fields. Position is clamped to the sensor and snapped to its step; a size is snapped down to its step and
 * clamped to the minimum and the sensor (the window slides back inside rather than refusing). With a locked shape, a width sets the height and a
 * height sets the width. `note` says in words what was changed, for the line under the field; null when the number was taken as typed.
 */
export function setField(cur: Rect, field: Field, raw: number, ratio: number | null, l: Limits, s: Sensor): FieldResult {
  const mn = minSize(l), mx = maxSize(l, s)
  const v = Math.round(raw)
  let next: Rect
  let note: string | null = null
  if (!Number.isFinite(raw)) return { rect: cur, note: 'Enter a whole number of pixels.' }
  if (field === 'x' || field === 'y') {
    const max = field === 'x' ? down(s.w - cur.w, l.step_x) : down(s.h - cur.h, l.step_y)
    const step = field === 'x' ? l.step_x : l.step_y
    const val = clamp(nearest(v, step), 0, Math.max(0, max))
    if (v < 0 || v > max) note = `${field.toUpperCase()} is limited to 0 to ${max} for this size.`
    else if (val !== v) note = `${field.toUpperCase()} snaps to ${val} (steps of ${step}).`
    next = { ...cur, [field]: val }
    return { rect: snapRect(next, l, s), note }
  }
  if (ratio == null) {
    const step = field === 'w' ? l.step_w : l.step_h
    const lo = field === 'w' ? mn.w : mn.h, hi = field === 'w' ? mx.w : mx.h
    const val = clamp(down(v, step), Math.min(lo, hi), hi)
    const label = field === 'w' ? 'Width' : 'Height'
    if (v < lo) note = `${label} is at least ${lo} pixels.`
    else if (v > hi) note = `${label} is at most ${hi} pixels on this sensor.`
    else if (val !== v) note = `${label} snaps down to ${val} (steps of ${step}).`
    next = { ...cur, [field]: val }
    return { rect: snapRect(next, l, s), note }
  }
  // locked: the typed side decides, the other follows
  const wantW = field === 'w' ? v : v * ratio
  const sz = lockedSize(wantW, ratio, l, s)
  const label = field === 'w' ? 'Width' : 'Height'
  const got = field === 'w' ? sz.w : sz.h
  if (got !== v) note = `${label} became ${got} to keep the shape and the camera's steps.`
  return { rect: snapRect({ ...cur, w: sz.w, h: sz.h }, l, s), note }
}

/** The arrow keys: move by one step; with shift, grow or shrink by one step (the top-left corner stays, the shape holds when locked). */
export function keyRect(cur: Rect, key: string, shift: boolean, ratio: number | null, l: Limits, s: Sensor, big = false): Rect | null {
  const dir = ({ ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] } as Record<string, [number, number]>)[key]
  if (!dir) return null
  const m = big ? 10 : 1
  if (!shift) return snapRect({ ...cur, x: cur.x + dir[0] * st(l.step_x) * m, y: cur.y + dir[1] * st(l.step_y) * m }, l, s)
  const dw = dir[0] * st(l.step_w) * m, dh = dir[1] * st(l.step_h) * m
  if (ratio == null) return snapRect({ ...cur, w: cur.w + dw, h: cur.h + dh }, l, s)
  // locked: left/right change the width, up/down the height, and the other side follows
  const sz = dir[0] !== 0 ? lockedSize(cur.w + dw, ratio, l, s) : lockedSize((cur.h + dh) * ratio, ratio, l, s)
  return snapRect({ ...cur, w: sz.w, h: sz.h }, l, s)
}

/** The region centred on the sensor. */
export const centreRect = (r: Rect, l: Limits, s: Sensor): Rect => snapRect({ ...r, x: (s.w - r.w) / 2, y: (s.h - r.h) / 2 }, l, s)
/** The whole sensor, on the steps. */
export const fullRect = (l: Limits, s: Sensor): Rect => ({ x: 0, y: 0, ...maxSize(l, s) })
export const isFull = (r: Rect, s: Sensor, l?: Limits) => r.x === 0 && r.y === 0 && r.w >= (l ? maxSize(l, s).w : s.w) && r.h >= (l ? maxSize(l, s).h : s.h)
/** The centred half of the sensor, the starting point when there is no earlier region. */
export const halfRect = (l: Limits, s: Sensor): Rect => centreRect(snapRect({ x: 0, y: 0, w: s.w / 2, h: s.h / 2 }, l, s), l, s)

/** Is the region centred on the sensor (to within a step)? Then the picture's size label need not name an offset. */
export const isCentred = (r: Rect, s: Sensor, l?: Limits) => {
  const sx = l ? st(l.step_x) : 2, sy = l ? st(l.step_y) : 2
  return Math.abs(r.x - (s.w - r.w) / 2) <= sx && Math.abs(r.y - (s.h - r.h) / 2) <= sy
}

/** "1504 x 1504 at 752, 752" for the size label (the offset only when it is not centred), or just the size. */
export function regionLabel(r: Rect, s: Sensor, l?: Limits): string {
  if (isFull(r, s, l)) return `${r.w}×${r.h}`
  return isCentred(r, s, l) ? `${r.w}×${r.h}` : `${r.w}×${r.h} at ${r.x}, ${r.y}`
}

/** How much less data a frame carries than a full frame, as words: "about 4× less data per frame". */
export function dataNote(r: Rect, s: Sensor): string {
  const ratio = (s.w * s.h) / (r.w * r.h)
  if (ratio < 1.05) return 'the whole sensor, no saving'
  return `about ${ratio >= 10 ? Math.round(ratio) : Math.round(ratio * 10) / 10}× less data per frame`
}

/** Client pixels on the picture to sensor pixels (the picture fills its box). */
export const toSensor = (clientDelta: number, boxSize: number, sensorSize: number) => (clientDelta / boxSize) * sensorSize
