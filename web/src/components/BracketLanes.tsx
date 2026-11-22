import { api, Status } from '../api'
import { ErrorLine, Field, useAction } from './ui'
import { useSetting } from '../settings'

type Lane = { name: string; exposure_ms: string; gain: string; frames: string }

const PRESETS: Record<string, Lane[]> = {
  'Partial phase': [{ name: 'short', exposure_ms: '0.2', gain: '', frames: '200' }, { name: 'mid', exposure_ms: '1', gain: '', frames: '200' }],
  'Diamond ring': [{ name: 'e1', exposure_ms: '0.1', gain: '', frames: '100' }, { name: 'e2', exposure_ms: '1', gain: '', frames: '100' }, { name: 'e3', exposure_ms: '10', gain: '', frames: '100' }],
  'Totality': [{ name: 'corona1', exposure_ms: '2', gain: '', frames: '100' }, { name: 'corona2', exposure_ms: '10', gain: '', frames: '100' }, { name: 'corona3', exposure_ms: '40', gain: '', frames: '60' }, { name: 'earthshine', exposure_ms: '200', gain: '', frames: '20' }],
}

/** HDR bracketing: several exposures cycled in turn, each saved into its own folder. */
export function BracketLanes({ status }: { status: Status | null }) {
  const [json, setJson] = useSetting('capture.lanes', JSON.stringify(PRESETS['Partial phase']))
  const [cycles, setCycles] = useSetting('capture.lane_cycles', '')
  const { err, busy, run } = useAction()
  let lanes: Lane[] = []
  try { lanes = JSON.parse(json) } catch { lanes = PRESETS['Partial phase'] }
  const set = (l: Lane[]) => setJson(JSON.stringify(l))
  const edit = (i: number, k: keyof Lane, v: string) => set(lanes.map((x, j) => (j === i ? { ...x, [k]: v } : x)))
  const idle = status?.capture.state === 'idle'
  const start = () => run(() => api('POST', '/capture/lanes', {
    name: 'lanes',
    cycles: cycles ? parseInt(cycles, 10) : undefined,
    lanes: lanes.map((l) => ({ name: l.name, exposure_us: (parseFloat(l.exposure_ms) || 0) * 1000, gain: l.gain ? parseFloat(l.gain) : undefined, frames: parseInt(l.frames, 10) || undefined })),
  }))
  return (
    <div>
      <p className="hint-text">Cycles through the lanes in order: each lane sets its exposure, records its frames into its own folder (<code>lane-name/</code>) and moves on, repeating until you stop it. Frames right after each change are discarded. The exposure you had is restored at the end. For eclipses and the Moon; pick the exposures at the eyepiece first.</p>
      <div className="btnrow tight">
        {Object.keys(PRESETS).map((p) => <button key={p} type="button" className="grow" onClick={() => set(PRESETS[p])}>{p}</button>)}
      </div>
      {lanes.map((l, i) => (
        <div className="lane" key={i}>
          <Field label={`Lane ${i + 1} name`}>{(id) => <input id={id} value={l.name} onChange={(e) => edit(i, 'name', e.target.value)} />}</Field>
          <div className="btnrow tight">
            <Field label="Exposure, ms">{(id) => <input id={id} inputMode="decimal" value={l.exposure_ms} onChange={(e) => edit(i, 'exposure_ms', e.target.value)} />}</Field>
            <Field label="Gain (optional)">{(id) => <input id={id} inputMode="decimal" value={l.gain} onChange={(e) => edit(i, 'gain', e.target.value)} />}</Field>
            <Field label="Frames">{(id) => <input id={id} inputMode="numeric" value={l.frames} onChange={(e) => edit(i, 'frames', e.target.value)} />}</Field>
          </div>
          {lanes.length > 2 && <button type="button" onClick={() => set(lanes.filter((_, j) => j !== i))}>Remove lane {i + 1}</button>}
        </div>
      ))}
      <div className="btnrow tight">
        <button type="button" disabled={lanes.length >= 6} onClick={() => set([...lanes, { name: `lane${lanes.length + 1}`, exposure_ms: '1', gain: '', frames: '100' }])}>Add lane</button>
      </div>
      <Field label="Cycles (empty = until stopped)">{(id) => <input id={id} inputMode="numeric" value={cycles} onChange={(e) => setCycles(e.target.value)} />}</Field>
      <button type="button" className="primary wide" disabled={busy || !idle} onClick={start}>Start bracketing</button>
      <ErrorLine error={err} />
    </div>
  )
}
