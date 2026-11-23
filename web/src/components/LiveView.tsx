import React, { useRef, useState } from 'react'
import { api, Status, reportError } from '../api'
import { Display, cssFilter } from '../display'
import { HudLayer, HudMenu, useHudLayout } from './Hud'

/** Live image (MJPEG) with display filters, zoom/pan and a measurement overlay in frame-pixel coordinates. */
export function LiveView({ status, display, setDisplay, anchorPick, onPicked, onDisplay }: { status: Status | null; display: Display; setDisplay: (p: Partial<Display>) => void; anchorPick: boolean; onPicked: () => void; onDisplay: () => void }) {
  const box = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  const drag = useRef<{ x: number; y: number; px: number; py: number; moved: boolean } | null>(null)
  const [, force] = useState(0)
  const cam = status?.camera
  const t = status?.tracking
  const g = status?.guiding
  const sun = status?.sun
  const d = display
  const hud = useHudLayout()

  const clampPan = (zoom: number, px: number, py: number) => {
    const r = box.current!.getBoundingClientRect()
    const mx = (r.width * (zoom - 1)) / 2, my = (r.height * (zoom - 1)) / 2
    return { panX: Math.max(-mx, Math.min(mx, px)), panY: Math.max(-my, Math.min(my, py)) }
  }
  const onWheel = (e: React.WheelEvent) => {
    const zoom = Math.max(1, Math.min(8, d.zoom * (e.deltaY < 0 ? 1.15 : 1 / 1.15)))
    setDisplay({ zoom, ...clampPan(zoom, d.panX, d.panY) })
  }
  const onDown = (e: React.PointerEvent) => { drag.current = { x: e.clientX, y: e.clientY, px: d.panX, py: d.panY, moved: false } }
  const onMove = (e: React.PointerEvent) => {
    const s = drag.current
    if (!s || d.zoom <= 1) return
    const dx = e.clientX - s.x, dy = e.clientY - s.y
    if (Math.abs(dx) + Math.abs(dy) > 3) s.moved = true
    setDisplay(clampPan(d.zoom, s.px + dx, s.py + dy))
  }
  const onUp = async (e: React.PointerEvent) => {
    const s = drag.current
    drag.current = null
    if (!s || s.moved || !anchorPick || !cam) return
    const r = inner.current!.getBoundingClientRect() // already includes zoom/pan
    const x = ((e.clientX - r.left) / r.width) * cam.width
    const y = ((e.clientY - r.top) / r.height) * cam.height
    onPicked()
    try { await api('POST', '/tracking/start', { mode: 'anchor', x, y }) } catch (err) { alert((err as Error).message) }
  }

  if (!cam) return <div className="placeholder"><div><b>No camera open</b><br />Open the camera in the Camera tab to see the Sun.</div></div>
  const W = cam.width, H = cam.height
  const stroke = Math.max(1.5, W / 500) / d.zoom
  const cx = W / 2, cy = H / 2


  const matrix = (r: number, gc: number, b: number) => `${r} 0 0 0 0  ${gc} 0 0 0 0  ${b} 0 0 0 0  0 0 0 1 0`
  const hint = d.zoom > 1 ? `${d.zoom.toFixed(1)}×` : ''

  return (
    <div ref={box} className={`live ${anchorPick ? 'pick' : ''}`} style={{ '--ar': W / H } as React.CSSProperties}
      onWheel={onWheel} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onDoubleClick={() => setDisplay({ zoom: 1, panX: 0, panY: 0 })}>
      <svg width="0" height="0" style={{ position: 'absolute' }}>
        <filter id="rf-gamma" colorInterpolationFilters="sRGB">
          <feComponentTransfer>
            <feFuncR type="gamma" exponent={1 / d.gamma} /><feFuncG type="gamma" exponent={1 / d.gamma} /><feFuncB type="gamma" exponent={1 / d.gamma} />
          </feComponentTransfer>
        </filter>
        <filter id="rf-tint-none"><feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 1 0" /></filter>
        <filter id="rf-tint-orange" colorInterpolationFilters="sRGB"><feColorMatrix type="matrix" values={matrix(1, 0.5, 0.08)} /></filter>
        <filter id="rf-tint-red" colorInterpolationFilters="sRGB"><feColorMatrix type="matrix" values={matrix(1, 0.12, 0.05)} /></filter>
      </svg>
      <div ref={inner} className="zoomed" style={{ transform: `translate(${d.panX}px, ${d.panY}px) scale(${d.zoom})` }}>
        <img src={`/stream.mjpg?fps=${d.fps}`} alt="live view" draggable={false} style={{ filter: cssFilter(d) }} onLoad={() => force((n) => n + 1)} />
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet" style={d.overlays ? undefined : { display: 'none' }}>
          <g fill="none" strokeWidth={stroke}>
            {d.grid && <path stroke="#ff3b30" opacity={0.4} d={`M${W / 3} 0V${H}M${(2 * W) / 3} 0V${H}M0 ${H / 3}H${W}M0 ${(2 * H) / 3}H${W}`} />}
            {d.rings > 0 && (
              <g stroke="#ff3b30" opacity={0.9}>
                {Array.from({ length: d.rings }, (_, i) => <circle key={i} cx={cx} cy={cy} r={((i + 1) / d.rings) * (d.ringSize / 100) * H} />)}
              </g>
            )}
            {d.crosshair && <path stroke="#ff3b30" opacity={0.7} d={`M0 ${cy}H${W}M${cx} 0V${H}`} />}
            {(d.sunOutline || d.sunCentre) && sun && (
              <g stroke="#22d3ee" strokeWidth={stroke * 1.6}>
                {d.sunOutline && <circle cx={sun.pos.x} cy={sun.pos.y} r={sun.radius} strokeDasharray={sun.partial ? `${stroke * 4} ${stroke * 4}` : undefined} />}
                {d.sunCentre && <circle cx={sun.pos.x} cy={sun.pos.y} r={stroke * 3} fill="#22d3ee" />}
                {d.sunCentre && Math.hypot(sun.pos.x - cx, sun.pos.y - cy) > H * 0.02 && (
                  <>
                    <defs><marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="#fb923c" /></marker></defs>
                    <line x1={sun.pos.x} y1={sun.pos.y} x2={cx} y2={cy} stroke="#fb923c" strokeWidth={stroke * 2} markerEnd="url(#arrow)" />
                  </>
                )}
              </g>
            )}
            {g?.target && (
              <g stroke="#4ade80" strokeWidth={stroke * 1.6}>
                <circle cx={g.target[0]} cy={g.target[1]} r={W / 60} />
                <path d={`M${g.target[0] - W / 40} ${g.target[1]}h${W / 20}M${g.target[0]} ${g.target[1] - W / 40}v${W / 20}`} />
              </g>
            )}
            {t && (
              <g stroke={t.lost ? '#f87171' : '#fbbf24'} strokeWidth={stroke * 1.6}>
                {/* the tracked circle is the Sun outline (one model), so only the centre mark is drawn here */}
                {t.mode === 'anchor' && <rect x={t.x - 32} y={t.y - 32} width={64} height={64} />}
                <path d={`M${t.x - W / 50} ${t.y}h${W / 25}M${t.x} ${t.y - W / 50}v${W / 25}`} />
              </g>
            )}
          </g>
        </svg>
      </div>
      <HudLayer status={status} api={hud} previewFps={d.fps} />
      <div className="live-tools" onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
        <button type="button" aria-label="Zoom in" onClick={() => { const zoom = Math.min(8, d.zoom * 1.5); setDisplay({ zoom, ...clampPan(zoom, d.panX, d.panY) }) }}>+</button>
        <button type="button" aria-label="Zoom out" disabled={d.zoom <= 1} onClick={() => { const zoom = Math.max(1, d.zoom / 1.5); setDisplay({ zoom, ...clampPan(zoom, d.panX, d.panY) }) }}>−</button>
        <button type="button" aria-label="Prominence view" aria-pressed={!!status?.coronagraph?.enabled} className={status?.coronagraph?.enabled ? 'tool-on' : ''} disabled={!status?.sun && !status?.coronagraph?.enabled} title="Black out the Sun's disc to judge the exposure for prominences" onClick={() => api('PUT', '/coronagraph', { enabled: !status?.coronagraph?.enabled, scale: status?.coronagraph?.scale ?? 1.03 }).catch((e) => reportError(e, 'Prominence view'))}>
          <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><circle cx="10" cy="10" r="7.5" fill="none" stroke="currentColor" strokeWidth="2" /><circle cx="10" cy="10" r="4.5" fill="currentColor" /></svg>
        </button>
        <button type="button" aria-label={d.overlays ? 'Hide overlays' : 'Show overlays'} aria-pressed={!d.overlays} className={d.overlays ? '' : 'tool-on'} title="Hide the crosshair, rings and Sun marks to see the bare image" onClick={() => setDisplay({ overlays: !d.overlays })}>
          <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><path d="M1.5 10S5 4.5 10 4.5 18.5 10 18.5 10 15 15.5 10 15.5 1.5 10 1.5 10z" fill="none" stroke="currentColor" strokeWidth="2" /><circle cx="10" cy="10" r="2.5" fill="currentColor" />{d.overlays && <path d="M3 17L17 3" stroke="currentColor" strokeWidth="2" strokeLinecap="round" opacity="0" />}{!d.overlays && <path d="M3 17L17 3" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />}</svg>
        </button>
        <HudMenu api={hud} />
        <button type="button" aria-label="Display settings" onClick={onDisplay}>
          <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><path d="M3 5h8M15 5h2M3 10h2M9 10h8M3 15h8M15 15h2" stroke="currentColor" strokeWidth="2" strokeLinecap="round" fill="none" /><circle cx="13" cy="5" r="2" fill="currentColor" /><circle cx="7" cy="10" r="2" fill="currentColor" /><circle cx="13" cy="15" r="2" fill="currentColor" /></svg>
        </button>
      </div>
      <div className="hud">{W}×{H}{hint && ` · ${hint}`}</div>
      {anchorPick && <div className="hint" role="status">Tap a feature (sunspot, filament, plage) to track it</div>}
    </div>
  )
}
