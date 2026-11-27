import React, { useRef } from 'react'
import { Handle, HANDLES, keyRect, Limits, moveRect, Rect, resizeRect, Sensor } from '../region'

const NAMES: Record<Handle, string> = { nw: 'top left corner', n: 'top edge', ne: 'top right corner', e: 'right edge', se: 'bottom right corner', s: 'bottom edge', sw: 'bottom left corner', w: 'left edge' }
const POS: Record<Handle, [number, number]> = { nw: [0, 0], n: [0.5, 0], ne: [1, 0], e: [1, 0.5], se: [1, 1], s: [0.5, 1], sw: [0, 1], w: [0, 0.5] }

type Drag = { kind: 'move' | Handle; px: number; py: number; start: Rect; box: DOMRect }

/**
 * The selection drawn over the whole-sensor picture: the area outside the region dimmed, the region's outline, eight handles with 44 px hit areas
 * and a body you drag to move it. Everything is in sensor pixels (`rect`); the pixels on screen are only used to turn a pointer movement into
 * sensor pixels. It sits on the picture and draws on top whatever the picture shows, so it works on a black frame too.
 * Keys (when it has focus): arrows move by the camera's step, shift+arrows resize, Enter applies, Escape cancels.
 */
export function RegionSelect({ rect, limits, sensor, ratio, onChange, onConfirm, onCancel }: {
  rect: Rect
  limits: Limits
  sensor: Sensor
  /** width over height when the shape is locked */
  ratio: number | null
  onChange: (r: Rect) => void
  onConfirm: () => void
  onCancel: () => void
}) {
  const drag = useRef<Drag | null>(null)
  const root = useRef<HTMLDivElement>(null)
  const pct = (v: number, of: number) => `${(v / of) * 100}%`

  const begin = (kind: Drag['kind']) => (e: React.PointerEvent) => {
    if (e.button !== 0 && e.pointerType === 'mouse') return
    e.preventDefault()
    e.stopPropagation()
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    root.current?.focus({ preventScroll: true })
    drag.current = { kind, px: e.clientX, py: e.clientY, start: rect, box: root.current!.getBoundingClientRect() }
  }
  const move = (e: React.PointerEvent) => {
    const d = drag.current
    if (!d) return
    e.preventDefault()
    const dx = ((e.clientX - d.px) / d.box.width) * sensor.w
    const dy = ((e.clientY - d.py) / d.box.height) * sensor.h
    onChange(d.kind === 'move' ? moveRect(d.start, dx, dy, limits, sensor) : resizeRect(d.start, d.kind, dx, dy, ratio, limits, sensor))
  }
  const end = (e: React.PointerEvent) => {
    if (!drag.current) return
    drag.current = null
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId) } catch { /* already released */ }
  }
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); onConfirm(); return }
    if (e.key === 'Escape') { e.preventDefault(); onCancel(); return }
    const next = keyRect(rect, e.key, e.shiftKey, ratio, limits, sensor, e.altKey)
    if (next) { e.preventDefault(); onChange(next) }
  }

  const { x, y, w, h } = rect
  const hole = `M0 0H${sensor.w}V${sensor.h}H0Z M${x} ${y}V${y + h}H${x + w}V${y}Z`
  return (
    <div ref={root} className="region-sel" tabIndex={0} role="group" data-testid="region-select"
      aria-label={`Region ${rect.w} by ${rect.h} pixels at ${rect.x}, ${rect.y}. Arrow keys move it, shift and arrow keys resize it, Enter applies, Escape cancels.`}
      onKeyDown={onKey}>
      <svg viewBox={`0 0 ${sensor.w} ${sensor.h}`} preserveAspectRatio="none" aria-hidden="true">
        <path d={hole} fillRule="evenodd" className="region-dim" />
        <rect x={x} y={y} width={w} height={h} className="region-line-dark" vectorEffect="non-scaling-stroke" />
        <rect x={x} y={y} width={w} height={h} className="region-line" vectorEffect="non-scaling-stroke" />
      </svg>
      <div className="region-label" aria-hidden="true" data-testid="region-label">{w} × {h} at {x}, {y}</div>
      <div className="region-body" data-testid="region-body" style={{ left: pct(x, sensor.w), top: pct(y, sensor.h), width: pct(w, sensor.w), height: pct(h, sensor.h) }}
        onPointerDown={begin('move')} onPointerMove={move} onPointerUp={end} onPointerCancel={end} />
      {HANDLES.map((k) => {
        const [fx, fy] = POS[k]
        // the handle's centre stays 22 px inside the picture so its whole 44 px hit area is on the picture
        const at = (v: number, of: number) => `clamp(22px, ${pct(v, of)}, calc(100% - 22px))`
        return (
          <div key={k} className={`region-handle h-${k}`} data-testid={`handle-${k}`} role="presentation" aria-label={NAMES[k]}
            style={{ left: at(x + fx * w, sensor.w), top: at(y + fy * h, sensor.h) }}
            onPointerDown={begin(k)} onPointerMove={move} onPointerUp={end} onPointerCancel={end}>
            <i />
          </div>
        )
      })}
    </div>
  )
}
