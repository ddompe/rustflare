import { api, Status } from '../api'
import { ErrorLine, Field, useAction } from './ui'
import { useSetting } from '../settings'

/** Prominence video: raw SER lanes, the disc at the current exposure and a much longer exposure, cycled; no processing. */
export function ProminenceVideo({ status }: { status: Status | null }) {
  const [factor, setFactor] = useSetting('capture.promvid_factor', '8')
  const [frames, setFrames] = useSetting('capture.promvid_frames', '200')
  const [cycles, setCycles] = useSetting('capture.promvid_cycles', '')
  const [every, setEvery] = useSetting('capture.promvid_every', '')
  const { err, busy, run } = useAction()
  const idle = status?.capture.state === 'idle'
  const exp = status?.camera?.controls.find((c) => c.id === 'exposure_us')?.value
  const f = parseFloat(factor) || 8
  const start = () => run(() => api('POST', '/capture/lanes', {
    name: 'prom', kind: 'prominence',
    cycles: cycles ? parseInt(cycles, 10) : undefined,
    cycle_interval_s: every ? parseFloat(every) : undefined,
    lanes: [
      { name: 'disc', exposure_us: exp, frames: parseInt(frames, 10) || 200 },
      { name: 'prominence', exposure_us: (exp ?? 0) * f, frames: parseInt(frames, 10) || 200 },
    ],
  }))
  return (
    <div>
      <p className="hint-text">Records raw video in two lanes, <b>disc</b> at the current exposure{exp ? ` (${(exp / 1000).toFixed(2)} ms)` : ''} and <b>prominence</b> at a longer one, alternating until you stop it. Nothing is combined or stretched: every lane and cycle is its own short SER video in a <b>prominence_…</b> session folder, ready for lucky-imaging stacks that become the frames of a time-lapse. For single stills with a ready-made composite, use Prominence pair.</p>
      <Field label="Prominence exposure, times the disc">{(id) => <input id={id} inputMode="decimal" value={factor} onChange={(e) => setFactor(e.target.value)} />}</Field>
      <Field label="Frames per lane and turn">{(id) => <input id={id} inputMode="numeric" value={frames} onChange={(e) => setFrames(e.target.value)} />}</Field>
      <Field label="Cycles (empty = until stopped)">{(id) => <input id={id} inputMode="numeric" value={cycles} onChange={(e) => setCycles(e.target.value)} />}</Field>
      <Field label="Start a new cycle every, seconds (empty = back to back)">{(id) => <input id={id} inputMode="decimal" value={every} onChange={(e) => setEvery(e.target.value)} />}</Field>
      <button type="button" className="primary wide" disabled={busy || !idle || !exp} onClick={start}>Start prominence video</button>
      <ErrorLine error={err} />
    </div>
  )
}
