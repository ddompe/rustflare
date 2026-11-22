import { ReactNode } from 'react'
import { api, Status } from '../api'
import { Disclosure, Empty, Section, Segmented, useAction } from './ui'
import { useSetting } from '../settings'
import { GuideSettings } from './GuideSettings'

type Mode = 'limb' | 'anchor' | 'sun_disc'
const MODES: { id: Mode; label: string; help: string }[] = [
  { id: 'limb', label: 'Disc edge', help: 'Fits the visible solar edge (limb), so it works when only part of the disc is in frame, typical for H-alpha. Needs some limb in view.' },
  { id: 'anchor', label: 'Feature', help: 'Follow a sunspot, filament or plage. Use this when the field is entirely on the disc, with no limb in view.' },
  { id: 'sun_disc', label: 'Full disc', help: 'Centroid of the whole disc. Only when the whole disc is in frame.' },
]

const angle = (v: { x: number; y: number }) => `${((Math.atan2(v.y, v.x) * 180) / Math.PI).toFixed(0)}°`

function Check({ done, bad }: { done: boolean; bad?: boolean }) {
  return (
    <span className={`tick ${done ? 'done' : ''} ${bad ? 'bad' : ''}`} aria-hidden="true">
      {done && <svg viewBox="0 0 12 12" width="12" height="12"><path d="M2.5 6.5l2.5 2.5 4.5-5.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg>}
    </span>
  )
}

function Step({ done, bad, title, state, action, children }: { done: boolean; bad?: boolean; title: string; state: ReactNode; action?: ReactNode; children?: ReactNode }) {
  return (
    <li className="step">
      <Check done={done} bad={bad} />
      <div className="step-main">
        <div className="step-head"><b>{title}</b><span className={`step-state ${bad ? 'bad' : done ? 'ok' : ''}`}>{state}</span></div>
        {children}
        {action}
      </div>
    </li>
  )
}

/** Step 3: choose what to follow, calibrate once, then hold position. */
export function TrackTab({ status, anchorPick, setAnchorPick, goto, next }: { status: Status | null; anchorPick: boolean; setAnchorPick: (b: boolean) => void; goto: (t: 'aim') => void; next: () => void }) {
  const [mode, setMode] = useSetting<Mode>('tracking.mode', 'limb')
  const [radius, setRadius] = useSetting('tracking.radius', '')
  const { err, run } = useAction()
  const t = status?.tracking
  const g = status?.guiding
  const cam = status?.camera
  if (!cam || !status) return <Empty>Open the camera first: tracking follows the Sun in the live view.</Empty>

  const o = status.optics
  const sun = status.sun
  const mismatch = mode === 'limb' && sun && o.expected_radius_px && Math.abs(sun.radius / o.expected_radius_px - 1) > 0.1
  const start = () => {
    if (mode === 'anchor') return setAnchorPick(!anchorPick)
    return run(() => api('POST', '/tracking/start', { mode, radius: mode === 'limb' && radius ? parseFloat(radius) : undefined }))
  }
  const mountTracking = !!status.mount?.tracking
  const calibrated = !!g?.calibration
  const guiding = g?.state === 'guiding'
  const calibrating = g?.state === 'calibrating'
  const ready = !!t && !t.lost && calibrated && mountTracking && g?.state === 'off'

  if (!t) {
    return (
      <>
        <Section title="What should it follow?" error={err}>
          <Segmented label="Tracking target" value={mode} options={MODES} onChange={(m) => { setMode(m); setAnchorPick(false) }} />
          <p className="hint-text">{MODES.find((m) => m.id === mode)?.help}</p>
          {mode === 'limb' && (
            <Disclosure title="Disc radius" summary={radius ? `${radius} px` : status.optics.expected_radius_px ? `${status.optics.expected_radius_px.toFixed(0)} px from Optics` : 'automatic'}>
              <div className="field">
                <label htmlFor="trk-radius">Radius (px), leave blank to use Optics</label>
                <input id="trk-radius" inputMode="decimal" value={radius} onChange={(e) => setRadius(e.target.value)} placeholder={status.optics.expected_radius_px ? `${status.optics.expected_radius_px.toFixed(0)} (from optics)` : 'set Optics, or fit from first frame'} />
              </div>
            </Disclosure>
          )}
          {mismatch && <div className="warn">Measured radius {sun!.radius.toFixed(0)} px differs from the {o.expected_radius_px!.toFixed(0)} px expected from Optics: check focal length, Barlow/reducer and binning (More, Optics).</div>}
          <button type="button" className={`wide ${anchorPick ? '' : 'primary'}`} onClick={start}>
            {mode === 'anchor' ? (anchorPick ? 'Cancel picking' : 'Pick a feature…') : 'Start tracking'}
          </button>
          {anchorPick && <p className="lead">Tap a sunspot, filament or plage on the live view.</p>}
        </Section>
      </>
    )
  }

  return (
    <>
      <Section error={err}>
        <ol className="steps">
          <Step done title="Tracking" bad={t.lost} state={t.lost ? 'TARGET LOST' : 'following'}>
            <div className="hint-text">{t.mode} @ {t.x.toFixed(0)}, {t.y.toFixed(0)}{t.radius ? ` · r ${t.radius.toFixed(0)} px` : ''}</div>
          </Step>
          {!mountTracking && (
            <Step done={false} title="Mount solar tracking" state="off" action={<button type="button" className="small" onClick={() => goto('aim')}>Go to Aim</button>}>
              <div className="hint-text">Calibration and guiding need the mount's solar tracking on.</div>
            </Step>
          )}
          <Step
            done={calibrated}
            title="Calibrate"
            state={calibrating ? 'calibrating…' : calibrated ? 'done' : 'needed once'}
            action={
              <button type="button" className={`small ${!calibrated && mountTracking ? 'primary' : calibrated ? 'quiet' : ''}`} disabled={g?.state !== 'off' || !mountTracking} onClick={() => run(() => api('POST', '/guiding/calibrate'))}>
                {calibrating ? 'Calibrating…' : calibrated ? 'Recalibrate' : 'Calibrate'}
              </button>
            }
          >
            <div className="hint-text">{calibrated ? `RA+ moves the image ${angle(g!.calibration!.primary_dir)}, Dec+ ${angle(g!.calibration!.secondary_dir)}` : 'Teaches RustFlare which way each mount axis moves the image. Takes a few seconds per axis.'}</div>
          </Step>
          <Step done={guiding} title="Hold position" state={guiding ? <><b className="bignum">{g?.error_px?.toFixed(1) ?? '–'}</b> px error</> : 'not holding'}>
            {guiding ? (
              <button type="button" className="danger wide big" onClick={() => run(() => api('POST', '/guiding/stop'))}>Stop guiding</button>
            ) : (
              <button type="button" className={`wide big ${ready ? 'primary' : ''}`} disabled={!ready} onClick={() => run(() => api('POST', '/guiding/start', { center: false }))}>Hold position</button>
            )}
          </Step>
        </ol>
        {!guiding && !ready && <p className="hint-text">Hold position needs mount solar tracking ({mountTracking ? 'on' : 'off'}) and a calibration ({calibrated ? 'done' : 'not done yet'}){t.lost ? ' and a target in view' : ''}. Until then, centre by hand with the slew pad.</p>}
      </Section>
      <Disclosure title="Centre the disc instead">
        <p className="hint-text">Moves the disc centre to the middle of the frame and holds it there. For a partial disc you usually want <b>Hold position</b> instead.</p>
        <button type="button" className="wide" disabled={!ready} onClick={() => run(() => api('POST', '/guiding/start', { center: true }))}>Centre and hold</button>
      </Disclosure>
      <Disclosure title="Guiding settings" summary="drift only, not seeing">
        <GuideSettings status={status} />
      </Disclosure>
      <Disclosure title="When to recalibrate">
        <p className="hint-text">Redo it after rotating the camera, after a meridian flip, or (alt-az) as the Sun moves. It needs the mount's solar tracking on and moves each axis for a few seconds.</p>
      </Disclosure>
      <div className="btnrow end">
        <button type="button" className="quiet" onClick={() => run(() => api('POST', '/tracking/stop'))}>Stop tracking</button>
        <button type="button" className="primary" onClick={next}>Next: Record →</button>
      </div>
    </>
  )
}
