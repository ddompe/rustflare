import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, Switch, useAction } from './ui'

const age = (s: number) => (s < 90 ? `${Math.round(s)} s ago` : `${Math.round(s / 60)} min ago`)

/** History of the lucky score: the line you want to push up while turning the focuser. */
export function Graph({ history, best, small }: { history: [number, number][]; best: number | null; small?: boolean }) {
  const W = 300, H = 110, PAD = 6
  if (history.length < 2) return <div className="focus-graph empty" role="img" aria-label="Focus history: collecting the first seconds">Collecting the first seconds…</div>
  const t1 = history[history.length - 1][0]
  const t0 = Math.max(history[0][0], t1 - 180)
  const vals = history.map((h) => h[1])
  let lo = Math.min(...vals), hi = Math.max(...vals, best ?? 0)
  const span = Math.max(hi - lo, hi * 0.15, 0.05)
  lo = lo - span * 0.1
  hi = lo + span * 1.25
  const x = (t: number) => PAD + ((t - t0) / (t1 - t0)) * (W - 2 * PAD)
  const y = (v: number) => H - PAD - ((v - lo) / (hi - lo)) * (H - 2 * PAD)
  const pts = history.map((h) => `${x(h[0]).toFixed(1)},${y(h[1]).toFixed(1)}`).join(' ')
  const last = history[history.length - 1]
  return (
    <svg className={`focus-graph ${small ? "small" : ""}`} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Focus score history, latest ${last[1].toFixed(2)}, best ${best?.toFixed(2) ?? 'none yet'}`}>
      {best != null && <line x1={PAD} x2={W - PAD} y1={y(best)} y2={y(best)} className="fg-best" />}
      <polyline points={pts} className="fg-line" fill="none" />
      <circle cx={x(last[0])} cy={y(last[1])} r="4" className="fg-now" />
    </svg>
  )
}

/** Focus assist: a sharpness score of short bursts (lucky imaging) for turning the focuser by hand. */
export function FocusAssist({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  const f = status?.focus
  const [want, setWant] = useState<boolean | null>(null) // shown at once, until the status catches up
  useEffect(() => { if (want != null && !!f?.enabled === want) setWant(null) }, [f?.enabled, want])
  if (!status?.camera) return <p className="hint-text">Open the camera first.</p>
  const on = want ?? !!f?.enabled
  const h = f?.history ?? []
  const prev = h.slice(-4, -1).map((p) => p[1])
  const trend = f?.lucky != null && prev.length > 0 ? f.lucky / (prev.reduce((a, b) => a + b, 0) / prev.length) : null
  const verdict = trend == null ? '' : trend > 1.03 ? '▲ sharper' : trend < 0.97 ? '▼ softer' : '● steady'
  return (
    <div>
      <Switch label="Focus assist" checked={on} onChange={(v) => { setWant(v); run(() => api('PUT', '/focus', { enabled: v })) }}
        hint="Turn the focuser slowly. The score and its history also show on the live image." />
      {on && (
        <>
          <div className="focus-score" aria-live="polite">
            <div className="fs-main"><b>{f?.lucky != null ? f.lucky.toFixed(2) : '–'}</b><span className={`fs-trend ${trend != null && trend > 1.03 ? 'up' : trend != null && trend < 0.97 ? 'down' : ''}`}>{verdict}</span></div>
            <div className="muted">live {f?.live != null ? f.live.toFixed(2) : '–'} · best {f?.best != null ? f.best.toFixed(2) : '–'}{f?.best_age_s != null ? ` (${age(f.best_age_s)})` : ''}</div>
          </div>
          <Graph history={h as [number, number][]} best={f?.best ?? null} />
          {f?.note && <div className="warn-text" role="status">{f.note}</div>}
          <div className="hint-text">Each point is a burst of about a second that counts its sharpest quarter of the frames, so seeing does not fool it. Aim for the highest point of the line, then turn back to the dashed best line: a peak you pass and come back to is the focus. Keep tracking on so the Sun stays put; a changing exposure does not matter.</div>
          <button type="button" onClick={() => run(() => api('PUT', '/focus', { enabled: true, reset: true }))}>Reset history</button>
        </>
      )}
      <ErrorLine error={err} />
    </div>
  )
}

/** Compact score and history for the focus overlay widget (see Hud.tsx), so the focuser can be turned while watching the picture. */
export function FocusBody({ status }: { status: Status }) {
  const f = status.focus
  if (!f?.enabled) return null
  return (
    <div className="focus-body">
      <b>{f.lucky != null ? f.lucky.toFixed(2) : '…'}</b>
      <span>best {f.best != null ? f.best.toFixed(2) : '–'}</span>
      <div aria-hidden="true"><Graph history={f.history as [number, number][]} best={f.best ?? null} small /></div>
    </div>
  )
}
