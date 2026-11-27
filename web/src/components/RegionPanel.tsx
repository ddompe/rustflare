import { useEffect, useRef, useState } from 'react'
import { api, Status } from '../api'
import { centreRect, dataNote, Field, fullRect, halfRect, isFull, LockId, LOCKS, lockRatio, Limits, Rect, reshape, Sensor, setField } from '../region'

/** Why choosing a region is not possible right now, or null when it is. Shared by every button that starts it. */
export function regionWhy(status: Status | null): string | null {
  if (!status?.camera) return 'open a camera first.'
  if (!status.roi) return 'this camera cannot read out part of its sensor.'
  if (status.capture.state !== 'idle') return 'not while recording or capturing.'
  if (status.guiding?.state === 'calibrating') return 'not while calibrating.'
  return null
}

type Sel = { rect: Rect; prev: Rect; lock: LockId; ratio: number | null; notes: Partial<Record<Field, string>> }

export type RegionApi = {
  active: boolean
  /** the picture is the whole sensor, so the selection can be drawn on it */
  ready: boolean
  sel: Sel | null
  limits: Limits | null
  sensor: Sensor | null
  busy: boolean
  err: string | null
  /** one line to show after Apply */
  done: string | null
  start: () => void
  cancel: () => void
  apply: () => void
  update: (r: Rect) => void
  setLock: (id: LockId) => void
  edit: (f: Field, raw: number) => void
  centre: () => void
  full: () => void
  dismissDone: () => void
  fullFrameNow: () => void
}

const toRoiBody = (r: Rect) => ({ x: r.x, y: r.y, width: r.w, height: r.h })

/**
 * Choosing a region of interest. Starting puts the camera on its whole sensor (the earlier region is remembered), so the picture is what the
 * selection is drawn on. Apply sends the region and leaves; Cancel puts the earlier region back.
 */
export function useRegion(status: Status | null): RegionApi {
  const [sel, setSel] = useState<Sel | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)
  const roi = status?.roi
  const limits = roi?.limits ?? null
  const sensor = roi ? { w: roi.sensor_width, h: roi.sensor_height } : null
  const cam = status?.camera
  const ready = !!(sel && roi && sensor && cam && cam.width === sensor.w && cam.height === sensor.h && roi.width === sensor.w && roi.height === sensor.h)

  useEffect(() => { // the message after Apply goes by itself
    if (!done) return
    const t = window.setTimeout(() => setDone(null), 15_000)
    return () => window.clearTimeout(t)
  }, [done])

  // a capture starting while choosing ends the choice: the region cannot change under a recording
  const capturing = status != null && status.capture.state !== 'idle'
  useEffect(() => { if (capturing && sel) { setSel(null); setErr(null) } }, [capturing, sel])

  const start = () => {
    if (!roi || !limits || !sensor || sel || busy) return
    const prev: Rect = { x: roi.x, y: roi.y, w: roi.width, h: roi.height }
    const init = isFull(prev, sensor, limits) ? halfRect(limits, sensor) : prev
    setErr(null); setDone(null)
    setSel({ rect: init, prev, lock: 'free', ratio: null, notes: {} })
    if (!isFull(prev, sensor, limits)) {
      setBusy(true)
      api('PUT', '/camera/roi', { fraction: 1 })
        .catch((e) => { setSel(null); setErr((e as Error).message) })
        .finally(() => setBusy(false))
    }
  }

  const leave = async (restore: boolean) => {
    const s = sel
    if (!s || busy) return
    setBusy(true)
    try {
      if (restore && sensor && limits && !isFull(s.prev, sensor, limits)) await api('PUT', '/camera/roi', toRoiBody(s.prev))
      setSel(null); setErr(null)
    } catch (e) {
      setErr((e as Error).message)
    } finally { setBusy(false) }
  }

  const apply = async () => {
    if (!sel || busy || !ready) return
    setBusy(true)
    try {
      const eff = await api<{ x: number; y: number; width: number; height: number }>('PUT', '/camera/roi', toRoiBody(sel.rect))
      const r: Rect = { x: eff.x, y: eff.y, w: eff.width, h: eff.height }
      setDone(`Region set: ${r.w}×${r.h}${isFull(r, sensor!, limits!) ? '' : ` at ${r.x}, ${r.y}`}, ${dataNote(r, sensor!)}.`)
      setSel(null); setErr(null)
    } catch (e) {
      setErr((e as Error).message)
    } finally { setBusy(false) }
  }

  const patch = (rect: Rect, notes: Sel['notes'] = {}) => setSel((s) => (s ? { ...s, rect, notes } : s))

  return {
    active: !!sel, ready, sel, limits, sensor, busy, err, done,
    start,
    cancel: () => { void leave(true) },
    apply: () => { void apply() },
    update: (r) => patch(r),
    setLock: (id) => {
      if (!sel || !limits || !sensor) return
      const ratio = lockRatio(id, sel.rect, sensor)
      setSel({ ...sel, lock: id, ratio, rect: ratio == null ? sel.rect : reshape(sel.rect, ratio, limits, sensor), notes: {} })
    },
    edit: (f, raw) => {
      if (!sel || !limits || !sensor) return
      const r = setField(sel.rect, f, raw, sel.ratio, limits, sensor)
      patch(r.rect, r.note ? { [f]: r.note } : {})
    },
    centre: () => { if (sel && limits && sensor) patch(centreRect(sel.rect, limits, sensor)) },
    full: () => { if (sel && limits && sensor) { setSel({ ...sel, lock: 'free', ratio: null, rect: fullRect(limits, sensor), notes: {} }) } },
    dismissDone: () => setDone(null),
    fullFrameNow: () => { setDone(null); api('PUT', '/camera/roi', { fraction: 1 }).catch((e) => setErr((e as Error).message)) },
  }
}

/** The numeric fields, the aspect lock and the buttons. Replaces the tab's controls while choosing, under the picture and never over it. */
export function RegionPanel({ region }: { region: RegionApi }) {
  const { sel, limits, sensor } = region
  const [editing, setEditing] = useState<{ f: Field; text: string } | null>(null)
  const panel = useRef<HTMLDivElement>(null)
  if (!sel || !limits || !sensor) return null
  const { rect } = sel
  const value = (f: Field) => (editing?.f === f ? editing.text : String(f === 'x' ? rect.x : f === 'y' ? rect.y : f === 'w' ? rect.w : rect.h))
  const commit = (f: Field) => {
    if (editing?.f !== f) return
    const raw = editing.text.trim() === '' ? NaN : Number(editing.text)
    setEditing(null)
    region.edit(f, raw)
  }
  const FIELDS: { f: Field; label: string; step: number }[] = [
    { f: 'x', label: 'X', step: limits.step_x }, { f: 'y', label: 'Y', step: limits.step_y },
    { f: 'w', label: 'Width', step: limits.step_w }, { f: 'h', label: 'Height', step: limits.step_h },
  ]
  const notes = FIELDS.map((x) => sel.notes[x.f]).filter((n): n is string => !!n)
  const onKey = (e: React.KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); region.cancel() } }
  const waiting = !region.ready
  return (
    <div className="region-panel" ref={panel} onKeyDown={onKey} data-testid="region-panel">
      <h2 className="region-title">Choose the region</h2>
      <p className="hint-text region-help">{waiting ? 'Switching to the whole sensor…' : 'Drag the box to move it, a handle to resize it.'}</p>
      <div className="region-fields" role="group" aria-label="Region in sensor pixels">
        {FIELDS.map(({ f, label, step }) => (
          <div key={f} className="region-field">
            <label htmlFor={`region-${f}`}>{label}</label>
            <input id={`region-${f}`} type="text" inputMode="numeric" pattern="[0-9]*" autoComplete="off" value={value(f)} aria-describedby={sel.notes[f] ? 'region-notes' : undefined} aria-invalid={sel.notes[f] ? true : undefined}
              data-step={step}
              onFocus={(e) => { setEditing({ f, text: value(f) }); e.currentTarget.select() }}
              onChange={(e) => setEditing({ f, text: e.target.value.replace(/[^0-9-]/g, '') })}
              onBlur={() => commit(f)}
              onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); commit(f) } }} />
          </div>
        ))}
      </div>
      <div id="region-notes" className="region-notes" role="status" aria-live="polite">{notes.map((n) => <p key={n} className="warn-line"><b>Adjusted:</b> {n}</p>)}</div>
      <div className="region-lock">
        <label htmlFor="region-lock">Aspect lock</label>
        <select id="region-lock" value={sel.lock} onChange={(e) => region.setLock(e.target.value as LockId)}>
          {LOCKS.map((l) => <option key={l.id} value={l.id}>{l.label}</option>)}
        </select>
      </div>
      <div className="region-btns">
        <button type="button" onClick={region.centre}>Centre</button>
        <button type="button" onClick={region.full}>Full frame</button>
        <button type="button" onClick={region.cancel} disabled={region.busy}>Cancel</button>
      </div>
      <div className="region-apply">
        <p className="region-result" data-testid="region-result" role="status">Will read <b>{rect.w} × {rect.h}</b> at {rect.x}, {rect.y}; {dataNote(rect, sensor)}.</p>
        {region.err && <div className="err" role="alert">{region.err}</div>}
        <button type="button" className="primary" onClick={region.apply} disabled={waiting || region.busy} aria-describedby={waiting ? 'region-help-wait' : undefined}>Apply region</button>
        {waiting && <span id="region-help-wait" className="sr-only">Waiting for the whole-sensor picture.</span>}
      </div>
    </div>
  )
}

/** The line shown after Apply: what was set, and a way back to the whole sensor. */
export function RegionDone({ region }: { region: RegionApi }) {
  if (!region.done) return null
  return (
    <div className="region-done" role="status" data-testid="region-done">
      <span>{region.done}</span>
      <button type="button" onClick={region.fullFrameNow}>Full frame</button>
      <button type="button" className="quiet" onClick={region.dismissDone} aria-label="Dismiss the message">Dismiss</button>
    </div>
  )
}
