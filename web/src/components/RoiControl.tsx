import { api, Status } from '../api'
import { ErrorLine, Segmented, useAction } from './ui'

/** Region of interest: read out only the centre of the sensor for a higher frame rate. */
export function RoiControl({ status }: { status: Status | null }) {
  const { err, busy, run } = useAction()
  const r = status?.roi
  if (!r) return <p className="hint-text">This camera cannot report its sensor size, so a region of interest is not available.</p>
  const frac = r.width >= r.sensor_width ? '1' : String(Math.round((r.width / r.sensor_width) * 100) / 100)
  const known = ['1', '0.75', '0.5', '0.25']
  const idle = status?.capture.state === 'idle'
  return (
    <div>
      <Segmented label="Frame size" value={known.includes(frac) ? frac : '1'} onChange={(v) => { if (idle && !busy) run(() => api('PUT', '/camera/roi', { fraction: parseFloat(v) })) }}
        options={[{ id: '1', label: 'Full' }, { id: '0.75', label: '75 %' }, { id: '0.5', label: '50 %' }, { id: '0.25', label: '25 %' }]} />
      <div className="hint-text">Now {r.width}×{r.height} of {r.sensor_width}×{r.sensor_height}, centred. A smaller region reads out faster (higher fps over the same USB link) and makes smaller files. Changing it restarts tracking, and flats and darks must be retaken for the new size.{!idle ? ' Not while recording.' : ''}</div>
      <ErrorLine error={err} />
    </div>
  )
}
