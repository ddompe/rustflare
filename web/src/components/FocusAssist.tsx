import { useEffect, useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, Switch, useAction } from './ui'

const age = (s: number) => (s < 90 ? `${Math.round(s)} s ago` : `${Math.round(s / 60)} min ago`)

/** History of the lucky score: the line you want to push up while turning the focuser. */
export function Graph({ history, best, small }: { history: [number, number, number][]; best: number | null; small?: boolean }) {
  const W = 300, H = 110, PAD = 6
  if (history.length < 2) return <div className="focus-graph empty" role="img" aria-label="Focus history: collecting the first seconds">Collecting the first seconds…</div>
  const t1 = history[history.length - 1][0]
  const t0 = Math.max(history[0][0], t1 - 180)
  const vals = history.flatMap((h) => [h[1] - h[2], h[1] + h[2]])
  let lo = Math.min(...vals), hi = Math.max(...vals, best ?? 0)
  const span = Math.max(hi - lo, hi * 0.15, 0.05)
  lo = lo - span * 0.1
  hi = lo + span * 1.25
  const x = (t: number) => PAD + ((t - t0) / (t1 - t0)) * (W - 2 * PAD)
  const y = (v: number) => H - PAD - ((v - lo) / (hi - lo)) * (H - 2 * PAD)
  const pts = history.map((h) => `${x(h[0]).toFixed(1)},${y(h[1]).toFixed(1)}`).join(' ')
  const last = history[history.length - 1]
  // the error band: what a point could be if the seeing had been a little different; differences inside it are noise
  const band = history.map((h) => `${x(h[0]).toFixed(1)},${y(h[1] + h[2]).toFixed(1)}`).concat([...history].reverse().map((h) => `${x(h[0]).toFixed(1)},${y(h[1] - h[2]).toFixed(1)}`)).join(' ')
  return (
    <svg className={`focus-graph ${small ? "small" : ""}`} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Focus score history, latest ${last[1].toFixed(2)}, best ${best?.toFixed(2) ?? 'none yet'}`}>
      {best != null && <line x1={PAD} x2={W - PAD} y1={y(best)} y2={y(best)} className="fg-best" />}
      <polygon points={band} className="fg-band" />
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
  // a change counts only when it is bigger than the noise of the two bursts compared
  const prev = h.slice(-3, -1)
  const prevMean = prev.length ? prev.reduce((a, p) => a + p[1], 0) / prev.length : null
  const prevErr = prev.length ? Math.sqrt(prev.reduce((a, p) => a + p[2] * p[2], 0)) / prev.length : 0
  const diff = f?.lucky != null && prevMean != null ? f.lucky - prevMean : null
  const noise = diff != null ? Math.hypot(f?.lucky_err ?? 0, prevErr) : 0
  const trend = diff == null ? null : Math.abs(diff) <= noise ? 1 : diff > 0 ? 1.1 : 0.9
  const verdict = trend == null ? '' : trend > 1 ? 'Sharper' : trend < 1 ? 'Softer' : 'Same (within noise)'
  const atBest = f?.lucky != null && f.best != null && Math.abs(f.lucky - f.best) <= Math.hypot(f.lucky_err ?? 0, f.best_err ?? 0)
  return (
    <div>
      <Switch label="Focus assist" checked={on} onChange={(v) => { setWant(v); run(() => api('PUT', '/focus', { enabled: v })) }}
        hint="Turn the focuser slowly. The score and its history also show on the live image." />
      {on && (
        <>
          <div className="focus-score" aria-live="polite">
            <div className="fs-main"><b>{f?.lucky != null ? f.lucky.toFixed(2) : '–'}{f?.lucky_err != null && f.lucky_err > 0 ? <small> ± {f.lucky_err.toFixed(2)}</small> : null}</b><span className={`fs-trend ${trend != null && trend > 1 ? 'up' : trend != null && trend < 1 ? 'down' : ''}`}>{verdict}</span></div>
            <div className="muted">live {f?.live != null ? f.live.toFixed(2) : '–'} · best {f?.best != null ? f.best.toFixed(2) : '–'}{f?.best_age_s != null ? ` (${age(f.best_age_s)})` : ''}{atBest ? ' · you are at the best' : ''}</div>
          </div>
          <Graph history={h} best={f?.best ?? null} />
          {f?.note && <div className="warn-text" role="status">{f.note}</div>}
          <div className="hint-text">Each point is a burst of about 2.5 seconds that counts only its sharpest frames, so seeing does not fool it, and the grey band is how far a point could be off. Differences inside the band are noise: look for the broad top of the curve, then come back to the dashed best line (the peak of the smoothed curve). {status?.profile === 'planet'
            ? 'The planet is found in the frame by itself and its edge, bands and rings are scored, so it can drift or be moved without a reset. Keep it from clipping: a white, flat-topped planet scores lower than it should (shorten the exposure).'
            : "Only the Sun's surface detail is scored, in a window that stays where the Sun was when you switched this on, so the Sun drifting a little while you turn the focuser does not matter. After a Go to, press Reset history."}</div>
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
      <div aria-hidden="true"><Graph history={f.history} best={f.best ?? null} small /></div>
    </div>
  )
}
