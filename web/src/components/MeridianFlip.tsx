import { useState } from 'react'
import { api, Status } from '../api'
import { ErrorLine, RangeField, Switch, useAction } from './ui'
import { useSetting } from '../settings'
import { noun, profileOf, why } from '../profile'
import { Why } from './Profile'

const mins = (m: number) => { const a = Math.abs(m); return a >= 90 ? `${(a / 60).toFixed(1)} h` : `${Math.round(a)} min` }

/** Meridian flip assist: remembers where you pointed and where the Sun was, goes back there after the flip, and re-centres. */
export function MeridianFlip({ status }: { status: Status }) {
  const { err, busy, run } = useAction()
  const [ask, setAsk] = useState<null | 'flip' | 'restore'>(null)
  const sky = status.flip_target ?? status.sky_sun
  const prof = profileOf(status)
  const body = status.flip_target?.name ?? noun(prof)
  const offFlip = why(prof, 'flip') ?? (prof === 'planet' && !status.target ? 'Go to a planet once (Aim tab) so the flip knows which one to follow.' : null)
  const f = status.flip
  const last = status.last_flip as { pre?: { unix?: number; ra_hours?: number; sun_x?: number; sun_y?: number }; slew_s?: number | null; total_s?: number; image_turned_over?: boolean | null; post_error_px?: number | null; result?: string } | null | undefined
  const go = (restore: boolean) => run(async () => { await api('POST', '/mount/flip', { confirm: true, restore }); setAsk(null) })
  return (
    <div>
      {sky ? <p className="hint-text"><b>{sky.minutes_to_meridian >= 0 ? `The ${body} crosses the meridian in ${mins(sky.minutes_to_meridian)}.` : `The ${body} crossed the meridian ${mins(sky.minutes_to_meridian)} ago.`}</b> The mount's own limits decide how far it keeps tracking past it.</p> : <p className="hint-text">Set your site (More, Site) to see how long until the {body} crosses the meridian.</p>}
      <Why id="why-flip" text={offFlip} subject="Meridian flip" />
      <AutoFlip status={status} off={!!offFlip} />
      <p className="hint-text">Before the flip press <b>Record position</b>: it saves where the mount points, where the {body} is in the picture and the time. <b>Flip and re-centre</b> records, sends the mount to that position (it turns over if the mount decides to), measures how long that took, restores the {status.flip_target?.rate ?? 'solar'} tracking rate, then brings the {body} back to the same place in the picture, allowing for the image turning upside down. If you flip by hand (app or mount buttons), use <b>Restore after a manual flip</b> afterwards. A guiding calibration is needed to re-centre.</p>
      {f && <div className="hint-text" role="status">Flip in progress: {f.phase} ({Math.round(f.elapsed_s)} s). STOP cancels.</div>}
      <div className="btnrow tight">
        <button type="button" disabled={busy || !!f || !!offFlip} aria-describedby={offFlip ? 'why-flip' : undefined} onClick={() => run(() => api('POST', '/mount/flip', { record_only: true }))}>Record position</button>
      </div>
      {ask === null ? (
        <div className="btnrow">
          <button type="button" className="grow" disabled={busy || !!f || !!offFlip} aria-describedby={offFlip ? 'why-flip' : undefined} onClick={() => setAsk('flip')}>Flip and re-centre…</button>
          <button type="button" className="grow" disabled={busy || !!f || !last?.pre || !!offFlip} aria-describedby={offFlip ? 'why-flip' : undefined} onClick={() => setAsk('restore')}>Restore after a manual flip…</button>
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
          Recorded {last.pre.unix ? new Date(last.pre.unix * 1000).toLocaleTimeString() : ''}: {body} at {Math.round(last.pre.sun_x ?? 0)}, {Math.round(last.pre.sun_y ?? 0)} px.
          {last.total_s != null && <> Last flip: slew {last.slew_s != null ? `${last.slew_s.toFixed(0)} s` : '?'}, total {last.total_s.toFixed(0)} s, image {last.image_turned_over ? 'turned over' : 'not turned over'}{last.post_error_px != null ? `, ${last.post_error_px.toFixed(1)} px off at the end` : ''}.</>}
          {last.result && <> {last.result}</>}
        </div>
      )}
      <ErrorLine error={err} />
    </div>
  )
}

const STAGE: Record<string, string> = {
  waiting: 'Waiting for the meridian.',
  recorded: 'Position recorded. Flips after the meridian.',
  flipping: 'Flipping now.',
  done: 'Flipped for this crossing.',
}

/** Unattended flip: records the position before the meridian, then flips (and recalibrates if it was guiding). */
function AutoFlip({ status, off }: { status: Status; off: boolean }) {
  const { err, run } = useAction()
  const a = status.auto_flip
  const [record, setRecord] = useSetting<number>('autoflip.record', a?.record_min ?? 5)
  const [after, setAfter] = useSetting<number>('autoflip.after', a?.flip_min ?? 1)
  const put = (enabled: boolean, r = a?.record_min ?? record, f = a?.flip_min ?? after) => run(() => api('PUT', '/mount/autoflip', { enabled, record_min: r, flip_min: f }))
  return (
    <div className="autoflip">
      <Switch label="Flip automatically" checked={!!a?.enabled} disabled={!status.site || !status.mount || (off && !a?.enabled)} describedBy="why-flip" onChange={(on) => put(on)} hint={!status.site ? 'Set your site first (More, Site).' : !status.mount ? 'Connect the mount first.' : a?.enabled ? STAGE[a.stage] ?? '' : 'Unattended: records the position a few minutes before the meridian and flips just after it, or as soon as the mount stops tracking by itself. It stops a running capture, recalibrates if it was guiding, and does not resume the capture.'} />
      {a?.enabled && (
        <>
          <RangeField label="Record before the meridian" value={a.record_min} min={1} max={30} step={1} show={`${a.record_min} min`} onChange={(v) => { setRecord(v); put(true, v, a.flip_min) }} />
          <RangeField label="Flip after the meridian" value={a.flip_min} min={0} max={20} step={0.5} show={`${a.flip_min} min`} onChange={(v) => { setAfter(v); put(true, a.record_min, v) }} />
          <p className="hint-text">Set the mount's own meridian limit (ZWO app) so it keeps tracking past the meridian for longer than the flip delay, or leave it: if the mount stops by itself, the flip starts right then. The AM3 flipping on its own is not supported.</p>
        </>
      )}
      <ErrorLine error={err} />
    </div>
  )
}
