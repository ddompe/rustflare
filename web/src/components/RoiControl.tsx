import { api, Status } from '../api'
import { isCentred, isFull } from '../region'
import { RegionApi, regionWhy } from './RegionPanel'
import { ErrorLine, Segmented, useAction } from './ui'

/** Region of interest: read out only part of the sensor for a higher frame rate. The presets are centred and instant; Region... chooses any window by dragging. */
export function RoiControl({ status, region }: { status: Status | null; region: RegionApi }) {
  const { err, busy, run } = useAction()
  const r = status?.roi
  if (!r) return <p className="hint-text">This camera cannot report its sensor size, so a region of interest is not available.</p>
  const rect = { x: r.x, y: r.y, w: r.width, h: r.height }
  const sensor = { w: r.sensor_width, h: r.sensor_height }
  const centred = isCentred(rect, sensor, r.limits)
  const frac = isFull(rect, sensor, r.limits) ? '1' : String(Math.round((r.width / r.sensor_width) * 100) / 100)
  const known = ['1', '0.75', '0.5', '0.25']
  const idle = status?.capture.state === 'idle'
  const why = regionWhy(status)
  return (
    <div>
      <Segmented label="Frame size" value={centred && known.includes(frac) ? frac : ''} onChange={(v) => { if (idle && !busy) run(() => api('PUT', '/camera/roi', { fraction: parseFloat(v) })) }}
        options={[{ id: '1', label: 'Full' }, { id: '0.75', label: '75 %' }, { id: '0.5', label: '50 %' }, { id: '0.25', label: '25 %' }]} />
      <div className="btnrow">
        <button type="button" className="grow" disabled={!!why} aria-describedby={why ? 'why-region-cam' : undefined} onClick={region.start}>Region…</button>
      </div>
      {why && <div className="gate-why hint-text" id="why-region-cam" role="note"><b>Region…</b> is off: {why}</div>}
      <div className="hint-text">Now {r.width}×{r.height}{isFull(rect, sensor, r.limits) ? '' : centred ? ', centred' : ` at ${r.x}, ${r.y}`} of {r.sensor_width}×{r.sensor_height}. The presets are centred. Region… lets you drag any window over the whole sensor, with a size, an offset and an aspect lock. A smaller region reads out faster (higher fps over the same USB link) and makes smaller files. Changing it restarts tracking, and flats and darks must be retaken for the new size.{!idle ? ' Not while recording.' : ''}</div>
      <ErrorLine error={err} />
    </div>
  )
}
