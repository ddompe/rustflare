import { useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, useAction } from './ui'

const mins = (m: number) => { const a = Math.abs(m); return a >= 90 ? `${(a / 60).toFixed(1)} h` : `${Math.round(a)} min` }

/** Meridian flip assist: remembers where you pointed and where the Sun was, goes back there after the flip, and re-centres. */
export function MeridianFlip({ status }: { status: Status }) {
  const { err, busy, run } = useAction()
  const [ask, setAsk] = useState<null | 'flip' | 'restore'>(null)
  const sky = status.sky_sun
  const f = status.flip
  const last = status.last_flip as { pre?: { unix?: number; ra_hours?: number; sun_x?: number; sun_y?: number }; slew_s?: number | null; total_s?: number; image_turned_over?: boolean | null; post_error_px?: number | null; result?: string } | null | undefined
  const go = (restore: boolean) => run(async () => { await api('POST', '/mount/flip', { confirm: true, restore }); setAsk(null) })
  return (
    <div>
      {sky ? <p className="hint-text"><b>{sky.minutes_to_meridian >= 0 ? `The Sun crosses the meridian in ${mins(sky.minutes_to_meridian)}.` : `The Sun crossed the meridian ${mins(sky.minutes_to_meridian)} ago.`}</b> The mount's own limits decide how far it keeps tracking past it.</p> : <p className="hint-text">Set your site (More, Site) to see how long until the Sun crosses the meridian.</p>}
      <p className="hint-text">Before the flip press <b>Record position</b>: it saves where the mount points, where the Sun is in the picture and the time. <b>Flip and re-centre</b> records, sends the mount to that position (it turns over if the mount decides to), measures how long that took, then brings the Sun back to the same place in the picture, allowing for the image turning upside down. If you flip by hand (app or mount buttons), use <b>Restore after a manual flip</b> afterwards. A guiding calibration is needed to re-centre.</p>
      {f && <div className="hint-text" role="status">Flip in progress: {f.phase} ({Math.round(f.elapsed_s)} s). STOP cancels.</div>}
      <div className="btnrow tight">
        <button type="button" disabled={busy || !!f} onClick={() => run(() => api('POST', '/mount/flip', { record_only: true }))}>Record position</button>
      </div>
      {ask === null ? (
        <div className="btnrow">
          <button type="button" className="grow" disabled={busy || !!f} onClick={() => setAsk('flip')}>Flip and re-centre…</button>
          <button type="button" className="grow" disabled={busy || !!f || !last?.pre} onClick={() => setAsk('restore')}>Restore after a manual flip…</button>
        </div>
      ) : (
        <div className="confirm" role="alertdialog" aria-label="Meridian flip">
          <p>{ask === 'flip' ? 'The mount will slew (and may turn over). Make sure the telescope, cables and counterweight are clear on both sides.' : 'The mount will slew back to the recorded position. Make sure the telescope and cables are clear.'}</p>
          <div className="btnrow tight">
            <button type="button" className="grow" onClick={() => setAsk(null)}>Cancel</button>
            <button type="button" className="danger grow" disabled={busy} onClick={() => go(ask === 'restore')}>{ask === 'flip' ? 'Flip' : 'Restore'}</button>
          </div>
        </div>
      )}
      {last?.pre && (
        <div className="hint-text">
          Recorded {last.pre.unix ? new Date(last.pre.unix * 1000).toLocaleTimeString() : ''}: Sun at {Math.round(last.pre.sun_x ?? 0)}, {Math.round(last.pre.sun_y ?? 0)} px.
          {last.total_s != null && <> Last flip: slew {last.slew_s != null ? `${last.slew_s.toFixed(0)} s` : '?'}, total {last.total_s.toFixed(0)} s, image {last.image_turned_over ? 'turned over' : 'not turned over'}{last.post_error_px != null ? `, ${last.post_error_px.toFixed(1)} px off at the end` : ''}.</>}
          {last.result && <> {last.result}</>}
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}
