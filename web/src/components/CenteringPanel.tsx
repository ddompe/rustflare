import { api, Status } from '../api'
import { Panel, Row, useAction } from './ui'

/** Helps put the Sun in the middle of the frame: offset readout, which way to move, and one-click centre & guide. */
export function CenteringPanel({ status }: { status: Status | null }) {
  const { err, run } = useAction()
  const cam = status?.camera
  const sun = status?.sun
  if (!cam) return null

  const g = status?.guiding
  const dx = sun ? cam.width / 2 - sun.pos.x : 0 // image must move by (dx, dy) to centre the Sun
  const dy = sun ? cam.height / 2 - sun.pos.y : 0
  const dist = Math.hypot(dx, dy)
  const centered = !!sun && dist < Math.max(10, sun.radius * 0.03)

  // With a calibrated mount, say which pad buttons produce that image motion (primary+ = E, secondary+ = N).
  let pad: string | null = null
  const cal = g?.calibration
  if (cal && sun && !centered) {
    const p = cal.primary_dir, s = cal.secondary_dir
    const det = p.x * s.y - p.y * s.x
    if (Math.abs(det) > 1e-6) {
      const a = (s.y * dx - s.x * dy) / det // component along primary axis
      const b = (-p.y * dx + p.x * dy) / det // component along secondary axis
      pad = `${a >= 0 ? 'E' : 'W'} and ${b >= 0 ? 'N' : 'S'}`
    }
  }

  const arrow = (d: number, pos: string, neg: string) => (Math.abs(d) < 1 ? '' : d > 0 ? pos : neg)
  const ready = !!status?.tracking && !status.tracking.lost && !!g?.calibration && !!status?.mount?.tracking && g.state === 'off'

  return (
    <Panel title="Centering" error={err}>
      {!sun ? (
        <div className="muted">No Sun found in the frame (too dark? try more exposure).</div>
      ) : (
        <>
          <div className={centered ? 'ok-text big' : 'big'}>{centered ? 'Centred ✓' : `${dist.toFixed(0)} px off centre`}</div>
          <div className="muted small">
            Sun centre {sun.pos.x.toFixed(0)}, {sun.pos.y.toFixed(0)} · radius {sun.radius.toFixed(0)} px{sun.partial ? ' (partial disc, fitted)' : ''} · {(dist / sun.radius * 100).toFixed(0)}% of radius
          </div>
          {!centered && (
            <div>
              Move the image <b>{arrow(dx, '→', '←')}{Math.abs(dx) >= 1 ? ` ${Math.abs(dx).toFixed(0)} px` : ''}</b>{' '}
              <b>{arrow(dy, '↓', '↑')}{Math.abs(dy) >= 1 ? ` ${Math.abs(dy).toFixed(0)} px` : ''}</b>
              {pad && <span className="muted"> · pad: hold {pad}</span>}
            </div>
          )}
        </>
      )}
      {sun?.partial && sun.radius > Math.min(cam.width, cam.height) / 2 && (
        <div className="muted small">
          Only part of the disc is in view and its centre is {sun.pos.x < 0 || sun.pos.x > cam.width || sun.pos.y < 0 || sun.pos.y > cam.height ? 'outside' : 'near the edge of'} the frame. To frame a region of interest: slew it into view, then press <b>Hold position</b> (limb tracking keeps it there). "Centre &amp; guide" moves the disc <i>centre</i> to the middle of the frame, which is only useful if you want that.
        </div>
      )}
      <hr />
      <Row>
        <button className="primary" disabled={!ready} onClick={() => run(() => api('POST', '/guiding/start', { center: true }))}>Centre & guide</button>
        <button disabled={!ready} onClick={() => run(() => api('POST', '/guiding/start', { center: false }))}>Hold position</button>
      </Row>
      {!ready && g?.state === 'off' && (
        <div className="muted small">
          Needs: tracking started{!status?.tracking && ' ✗'}, mount sky tracking on{!status?.mount?.tracking && ' ✗'}, calibration{!g?.calibration && ' ✗'}. Until then, centre by hand with the slew pad.
        </div>
      )}
    </Panel>
  )
}
