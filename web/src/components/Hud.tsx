import { KeyboardEvent, PointerEvent, ReactNode, useEffect, useRef, useState } from 'react'
import { Status } from '../api'
import { useSetting } from '../settings'

/**
 * Overlay widgets on top of the live view: recording, RAM buffer, guiding (with an error sparkline), histogram, and a
 * fps/exposure/gain strip. Each can be switched on or off, dragged anywhere inside the picture, and minimised to one line
 * (tap it). Layout is saved with useSetting, separately for phone-sized and wide screens.
 *
 * Position is stored as two fractions (fx, fy) of the free space, so a widget always stays inside the view whatever its
 * size or the screen shape: left = fx * (W - w), top = fy * (H - h). Widgets only react to their own pointer events and
 * stop them there, so dragging one never pans the image and a drag on the picture never moves one.
 */
export type HudId = 'rec' | 'buffer' | 'guide' | 'hist' | 'strip'
interface W { on: boolean; fx: number; fy: number; min: boolean }
type Layout = Record<HudId, W>

export const HUD_ITEMS: { id: HudId; label: string; short: string; when: string }[] = [
  { id: 'rec', label: 'Recording', short: 'Recording', when: 'shows while recording' },
  { id: 'buffer', label: 'RAM buffer and disk', short: 'Buffer', when: 'shows while recording' },
  { id: 'guide', label: 'Guiding error', short: 'Guiding', when: 'shows while tracking' },
  { id: 'hist', label: 'Histogram', short: 'Histogram', when: '' },
  { id: 'strip', label: 'Status: fps, disk, exposure, mount, flats', short: 'Status', when: '' },
]

/** Corners and edges, so nothing sits over the middle of the picture by default. Phones start with none. */
const defaults = (on: boolean): Layout => ({
  guide: { on, fx: 0, fy: 0, min: false },
  rec: { on, fx: 0, fy: 1, min: false },
  buffer: { on, fx: 1, fy: 0.9, min: false },
  hist: { on: false, fx: 0, fy: 0.3, min: false },
  strip: { on: true, fx: 0.5, fy: 0, min: !on },
})

const small = () => typeof matchMedia === 'function' && matchMedia('(max-width: 699px), (max-height: 520px)').matches

export interface HudApi {
  get: (id: HudId) => W
  toggle: (id: HudId) => void
  move: (id: HudId, fx: number, fy: number) => void
  setMin: (id: HudId, min: boolean) => void
  reset: () => void
  opacity: number
  setOpacity: (v: number) => void
}

export function useHudLayout(): HudApi {
  const phone = useRef(small()).current
  const [layout, setLayout] = useSetting<Layout>(phone ? 'hud.layout.phone' : 'hud.layout.wide', defaults(!phone))
  const [opacity, setOpacity] = useSetting<number>('hud.opacity', 0.55)
  const def = defaults(!phone)
  const get = (id: HudId): W => ({ ...def[id], ...layout[id] })
  const patch = (id: HudId, p: Partial<W>) => setLayout({ ...layout, [id]: { ...get(id), ...p } })
  return { get, toggle: (id) => patch(id, { on: !get(id).on }), move: (id, fx, fy) => patch(id, { fx, fy }), setMin: (id, min) => patch(id, { min }), reset: () => { setLayout(defaults(!phone)); setOpacity(0.55) }, opacity, setOpacity }
}

const clamp = (v: number) => Math.max(0, Math.min(1, v))

function Widget({ id, label, w, api, mini, children }: { id: HudId; label: string; w: W; api: HudApi; mini: ReactNode; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef<{ sx: number; sy: number; left: number; top: number; rx: number; ry: number; moved: boolean; fx: number; fy: number } | null>(null)
  const setVars = (fx: number, fy: number) => { ref.current?.style.setProperty('--fx', String(fx)); ref.current?.style.setProperty('--fy', String(fy)) }
  const down = (e: PointerEvent) => {
    e.stopPropagation()
    const el = ref.current!, layer = el.parentElement!.getBoundingClientRect(), r = el.getBoundingClientRect()
    drag.current = { sx: e.clientX, sy: e.clientY, left: r.left - layer.left, top: r.top - layer.top, rx: layer.width - r.width, ry: layer.height - r.height, moved: false, fx: w.fx, fy: w.fy }
    try { el.setPointerCapture(e.pointerId) } catch { /* synthetic pointer */ }
  }
  const move = (e: PointerEvent) => {
    const d = drag.current
    if (!d) return
    e.stopPropagation()
    const dx = e.clientX - d.sx, dy = e.clientY - d.sy
    if (!d.moved && Math.hypot(dx, dy) < 6) return
    d.moved = true
    d.fx = d.rx > 0 ? clamp((d.left + dx) / d.rx) : 0
    d.fy = d.ry > 0 ? clamp((d.top + dy) / d.ry) : 0
    setVars(d.fx, d.fy)
  }
  const up = (e: PointerEvent) => {
    const d = drag.current
    drag.current = null
    if (!d) return
    e.stopPropagation()
    if (d.moved) api.move(id, d.fx, d.fy)
    else api.setMin(id, !w.min)
  }
  const cancel = () => { drag.current = null; setVars(w.fx, w.fy) }
  const key = (e: KeyboardEvent) => {
    const step = 0.05
    const d: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); api.setMin(id, !w.min) }
    else if (d[e.key]) { e.preventDefault(); api.move(id, clamp(w.fx + d[e.key][0]), clamp(w.fy + d[e.key][1])) }
  }
  return (
    <div
      ref={ref}
      className={`hud-w ${w.min ? 'min' : ''}`}
      style={{ '--fx': w.fx, '--fy': w.fy } as React.CSSProperties}
      role="button"
      tabIndex={0}
      aria-expanded={!w.min}
      aria-label={`${label} overlay. Tap to ${w.min ? 'expand' : 'minimise'}, drag or use arrow keys to move.`}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={cancel}
      onKeyDown={key}
      onWheel={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      {w.min ? mini : children}
    </div>
  )
}

/** Rolling guiding error, one sample about every 700 ms for a minute. */
function useErrorHistory(status: Status | null): number[] {
  const [h, setH] = useState<number[]>([])
  const last = useRef(0)
  const g = status?.guiding
  useEffect(() => {
    if (g?.state !== 'guiding' || g.error_px == null) { if (h.length) setH([]); return }
    const now = performance.now()
    if (now - last.current < 700) return
    last.current = now
    setH((x) => [...x.slice(-85), g.error_px as number])
  }, [g?.state, g?.error_px]) // eslint-disable-line react-hooks/exhaustive-deps
  return h
}

function Spark({ v }: { v: number[] }) {
  if (v.length < 2) return <div className="spark-empty">collecting…</div>
  const max = Math.max(1, ...v)
  const pts = v.map((y, i) => `${((i / 85) * 100).toFixed(1)},${(26 - (y / max) * 24).toFixed(1)}`).join(' ')
  return (
    <svg className="spark" viewBox="0 0 100 28" preserveAspectRatio="none" role="img" aria-label={`Guiding error over the last minute, up to ${max.toFixed(1)} pixels`}>
      <polyline points={pts} fill="none" stroke="#62e38f" strokeWidth="1.6" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

const expo = (us: number) => (us >= 1e6 ? `${(us / 1e6).toFixed(2)} s` : us >= 1000 ? `${(us / 1000).toFixed(2)} ms` : `${Math.round(us)} µs`)

export function HudLayer({ status, api, previewFps }: { status: Status | null; api: HudApi; previewFps?: number }) {
  const errHist = useErrorHistory(status)
  const cam = status?.camera
  const c = status?.capture
  if (!status || !cam || !c) return null
  const g = status.guiding
  const t = status.tracking
  const recording = c.state !== 'idle'
  const tracking = !!t || g.state !== 'off'
  const out: ReactNode[] = []
  const w = (id: HudId) => api.get(id)

  if (w('rec').on && recording) {
    out.push(
      <Widget key="rec" id="rec" label="Recording" w={w('rec')} api={api}
        mini={<span><span className="rec-dot" aria-hidden="true" /> REC <b className="cv">{c.done}{c.total != null ? `/${c.total}` : ''}</b></span>}>
        <div><span className="rec-dot" aria-hidden="true" /> <b>{c.state === 'flushing' ? 'Saving' : c.state === 'flats' ? 'Flats' : 'Recording'}</b> <span className="muted">{c.state}</span></div>
        <div><b className="cv">{c.done}{c.total != null ? `/${c.total}` : ''}</b> <span className="cl">frames</span></div>
        {(c.burst != null || c.next_in_s != null) && <div className="muted">{c.burst != null && `burst ${c.burst}${c.bursts_total ? `/${c.bursts_total}` : ''}`}{c.next_in_s != null && ` · next in ${c.next_in_s.toFixed(0)} s`}</div>}
        {c.file && <div className="hud-file">{c.file}</div>}
      </Widget>,
    )
  }
  if (w('buffer').on && recording && c.state !== 'flats') {
    const pct = c.buffer_capacity ? Math.round((c.buffer_queued / c.buffer_capacity) * 100) : 0
    out.push(
      <Widget key="buffer" id="buffer" label="RAM buffer" w={w('buffer')} api={api}
        mini={<span>Buffer <b className="cv">{pct}%</b></span>}>
        <div><b>Buffer</b> {c.state === 'flushing' && <span className="warn-text">flushing</span>}</div>
        <progress max={c.buffer_capacity || 1} value={c.buffer_queued} className="buf" aria-label="RAM buffer in use" />
        <div className="muted">{c.buffer_queued}/{c.buffer_capacity} frames</div>
        {(c.dropped ?? 0) > 0 && <div className="bad-text"><b>{c.dropped}</b> dropped</div>}
        <div><b className="cv">{c.disk_mb_s.toFixed(0)}</b> <span className="cl">MB/s to disk</span></div>
      </Widget>,
    )
  }
  if (w('guide').on && tracking) {
    const lost = !!t?.lost
    const label = lost ? 'Lost' : g.state === 'guiding' ? 'Holding' : g.state === 'calibrating' ? 'Calibrating' : t ? `Tracking ${t.mode}` : 'Idle'
    out.push(
      <Widget key="guide" id="guide" label="Guiding" w={w('guide')} api={api}
        mini={<span className={lost ? 'bad-text' : ''}>{lost ? 'LOST' : g.state === 'guiding' ? <>Hold <b className="cv">{g.error_px?.toFixed(1) ?? '–'}</b> px</> : label}</span>}>
        <div className={lost ? 'hud-lost' : ''}><b>{label}</b></div>
        {g.state === 'guiding' && (
          <>
            <div><b className="cv">{g.error_px?.toFixed(1) ?? '–'}</b> <span className="cl">px error (averaged)</span></div>
            <Spark v={errHist} />
          </>
        )}
      </Widget>,
    )
  }
  if (w('hist').on && cam.histogram.length > 0) {
    const max = Math.max(...cam.histogram.map(Math.sqrt), 1e-6)
    const clipped = cam.saturated > 0.001
    out.push(
      <Widget key="hist" id="hist" label="Histogram" w={w('hist')} api={api}
        mini={<span className={clipped ? 'bad-text' : ''}>{clipped ? <>Clip <b className="cv">{(cam.saturated * 100).toFixed(1)}%</b></> : 'No clipping'}</span>}>
        <svg className="hud-hist" viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true">
          {cam.histogram.map((b, i, a) => <rect key={i} x={(i * 100) / a.length} y={30 - (Math.sqrt(b) / max) * 30} width={(100 / a.length) * 0.9} height={(Math.sqrt(b) / max) * 30} fill={i === a.length - 1 && clipped ? '#ff7466' : '#7dd3fc'} />)}
        </svg>
        <div className={clipped ? 'bad-text' : 'ok'}>{clipped ? `Clipping ${(cam.saturated * 100).toFixed(1)}%` : 'No clipping'}</div>
      </Widget>,
    )
  }
  if (w('strip').on) {
    const e = cam.controls.find((x) => x.id === 'exposure_us'), gn = cam.controls.find((x) => x.id === 'gain')
    const m = status.mount
    const mountText = !m ? null : m.slewing ? 'slewing' : m.park === 'parked' ? 'parked' : m.tracking ? 'tracking' : m.at_home ? 'at home' : 'stopped'
    const flats = status.flats?.applied
    const shown = previewFps ? Math.min(previewFps, cam.fps) : cam.fps
    out.push(
      <Widget key="strip" id="strip" label="Status" w={w('strip')} api={api}
        mini={<span><b className="cv">{cam.fps.toFixed(0)}</b> fps{recording && <> · <b className="cv">{c.disk_mb_s.toFixed(0)}</b> MB/s</>}</span>}>
        <dl className="hud-grid">
          <dt>Camera</dt><dd><b className="cv">{cam.fps.toFixed(1)}</b> fps</dd>
          <dt>Screen</dt><dd><b className="cv">{shown.toFixed(0)}</b> fps</dd>
          <dt>Disk</dt><dd>{recording ? <><b className="cv">{c.disk_mb_s.toFixed(0)}</b> MB/s</> : <span className="muted">idle</span>}</dd>
          {e && <><dt>Exposure</dt><dd><b>{expo(e.value)}</b></dd></>}
          {gn && <><dt>Gain</dt><dd><b>{Math.round(gn.value)}</b></dd></>}
          {cam.temperature_c != null && <><dt>Sensor</dt><dd><b>{cam.temperature_c.toFixed(1)}</b> °C</dd></>}
          {mountText && <><dt>Mount</dt><dd><b>{mountText}</b></dd></>}
          <dt>Flat</dt><dd className={flats ? 'ok' : 'muted'}>{flats ? 'applied' : 'none'}</dd>
          <dt>Dark</dt><dd className={status.darks?.applied ? 'ok' : 'muted'}>{status.darks?.applied ? (status.darks.matches_camera ? 'applied' : 'mismatch') : 'none'}</dd>
        </dl>
      </Widget>,
    )
  }
  return <div className="hud-layer" style={{ '--hud-a': api.opacity } as React.CSSProperties}>{out}</div>
}

/** The menu button (for the live-view tool column) and its popover with one switch per widget. */
export function HudMenu({ api }: { api: HudApi }) {
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const away = (e: Event) => { if (!box.current?.contains(e.target as Node)) setOpen(false) }
    const esc = (e: globalThis.KeyboardEvent) => { if (e.key === 'Escape') setOpen(false) }
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', esc)
    return () => { document.removeEventListener('pointerdown', away); document.removeEventListener('keydown', esc) }
  }, [open])
  return (
    <div ref={box} className="hud-menu-wrap">
      <button type="button" aria-label="Overlay widgets" aria-expanded={open} aria-haspopup="true" onClick={() => setOpen(!open)}>
        <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><rect x="2.5" y="2.5" width="7" height="7" rx="1.5" fill="none" stroke="currentColor" strokeWidth="2" /><rect x="11.5" y="2.5" width="6" height="7" rx="1.5" fill="currentColor" /><rect x="2.5" y="11.5" width="7" height="6" rx="1.5" fill="currentColor" /><rect x="11.5" y="11.5" width="6" height="6" rx="1.5" fill="none" stroke="currentColor" strokeWidth="2" /></svg>
      </button>
      {open && (
        <div className="hud-menu" role="dialog" aria-label="Overlay widgets">
          <b>Overlay widgets</b>
          <div className="hud-toggles">
            {HUD_ITEMS.map((i) => (
              <button key={i.id} type="button" aria-pressed={api.get(i.id).on} className={api.get(i.id).on ? 'on' : ''} title={i.when || undefined} onClick={() => api.toggle(i.id)}>
                <span className="hud-check" aria-hidden="true">{api.get(i.id).on ? '✓' : ''}</span>{i.short}
              </button>
            ))}
          </div>
          <div className="ctl"><div className="ctl-head"><label htmlFor="hud-op">Opacity</label><output htmlFor="hud-op" className="ctl-val">{Math.round(api.opacity * 100)}%</output></div>
            <input id="hud-op" type="range" min={0.2} max={1} step={0.05} value={api.opacity} onChange={(e) => api.setOpacity(parseFloat(e.target.value))} /></div>
          <p className="hint-text">Drag to move, tap to shrink to one line. Recording and guiding widgets appear when there is something to show.</p>
          <button type="button" className="wide" onClick={api.reset}>Reset layout</button>
        </div>
      )}
    </div>
  )
}
