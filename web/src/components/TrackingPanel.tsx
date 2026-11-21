import { useState } from 'react'
import { api, Status } from '../api'
import { Panel, Row, useAction } from './ui'

const MODES = [
  { id: 'limb', label: 'Disc edge (limb): whole or partial disc', help: 'Fits the visible solar edge, so it works when only part of the disc is in frame (typical for H-alpha). Needs some limb in view.' },
  { id: 'anchor', label: 'Feature (anchor)', help: 'Follow a sunspot, filament or plage. Use this when the field is entirely on the disc, with no limb in view.' },
  { id: 'sun_disc', label: 'Full disc (centroid)', help: 'Whole disc in frame only.' },
]

const angle = (v: { x: number; y: number }) => `${((Math.atan2(v.y, v.x) * 180) / Math.PI).toFixed(0)}°`

export function TrackingPanel({ status, anchorPick, setAnchorPick }: { status: Status | null; anchorPick: boolean; setAnchorPick: (b: boolean) => void }) {
  const [mode, setMode] = useState('limb')
  const [radius, setRadius] = useState('')
  const { err, run } = useAction()
  const t = status?.tracking
  const g = status?.guiding
  const cam = status?.camera

  const start = () => {
    if (mode === 'anchor') return setAnchorPick(!anchorPick)
    return run(() => api('POST', '/tracking/start', { mode, radius: mode === 'limb' && radius ? parseFloat(radius) : undefined }))
  }

  return (
    <Panel title="Tracking & guiding" error={err}>
      {!cam ? <span className="muted">Open a camera first</span> : (
        <>
          <Row label="Target">
            <select value={mode} onChange={(e) => { setMode(e.target.value); setAnchorPick(false) }} disabled={!!t}>
              {MODES.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
            </select>
          </Row>
          <div className="muted small">{MODES.find((m) => m.id === mode)?.help}</div>
          {mode === 'limb' && !t && (
            <Row label="Radius (px)"><input value={radius} onChange={(e) => setRadius(e.target.value)} placeholder={status?.optics.expected_radius_px ? `${status.optics.expected_radius_px.toFixed(0)} (from optics)` : 'set Optics, or fit from first frame'} /></Row>
          )}
          {!t ? (
            <Row><button className="primary" onClick={start}>{mode === 'anchor' ? (anchorPick ? 'Cancel pick' : 'Pick feature…') : 'Start tracking'}</button></Row>
          ) : (
            <>
              <Row>
                <span className={t.lost ? 'err' : 'muted'}>
                  {t.lost ? 'TARGET LOST' : `${t.mode} @ ${t.x.toFixed(0)}, ${t.y.toFixed(0)}`}{t.radius ? ` · r ${t.radius.toFixed(0)} px` : ''}
                </span>
              </Row>
              <Row><button onClick={() => run(() => api('POST', '/tracking/stop'))}>Stop tracking</button></Row>
              <hr />
              <Row>
                <span className="muted">Calibration: {g?.calibration ? `RA+ moves image ${angle(g.calibration.primary_dir)}, Dec+ ${angle(g.calibration.secondary_dir)}` : 'not done'}</span>
                <button disabled={g?.state !== 'off'} onClick={() => run(() => api('POST', '/guiding/calibrate'))}>
                  {g?.state === 'calibrating' ? 'Calibrating…' : 'Calibrate'}
                </button>
              </Row>
              <Row>
                {g?.state === 'guiding' ? (
                  <>
                    <span className="ok-text">Guiding · error {g.error_px?.toFixed(1) ?? '–'} px</span>
                    <button onClick={() => run(() => api('POST', '/guiding/stop'))}>Stop guiding</button>
                  </>
                ) : (
                  <button className="primary" disabled={!g?.calibration || g.state !== 'off'} onClick={() => run(() => api('POST', '/guiding/start'))}>Start guiding</button>
                )}
              </Row>
              <div className="muted small">Calibration teaches Rustflare which way each mount axis moves the image. It needs the mount's solar tracking on and moves each axis for a few seconds. Redo it after rotating the camera, after a meridian flip, or (alt-az) as the Sun moves.</div>
            </>
          )}
        </>
      )}
    </Panel>
  )
}
