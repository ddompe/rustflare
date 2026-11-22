import { Field } from './ui'

export type Lane = { name: string; exposure_ms: string; gain: string; frames: string }

export const PRESETS: Record<string, Lane[]> = {
  'Partial phase': [{ name: 'short', exposure_ms: '0.2', gain: '', frames: '200' }, { name: 'mid', exposure_ms: '1', gain: '', frames: '200' }],
  'Diamond ring': [{ name: 'e1', exposure_ms: '0.1', gain: '', frames: '100' }, { name: 'e2', exposure_ms: '1', gain: '', frames: '100' }, { name: 'e3', exposure_ms: '10', gain: '', frames: '100' }],
  'Totality': [{ name: 'corona1', exposure_ms: '2', gain: '', frames: '100' }, { name: 'corona2', exposure_ms: '10', gain: '', frames: '100' }, { name: 'corona3', exposure_ms: '40', gain: '', frames: '60' }, { name: 'earthshine', exposure_ms: '200', gain: '', frames: '20' }],
}

export const parseLanes = (json: string): Lane[] => { try { const l = JSON.parse(json); return Array.isArray(l) ? l : PRESETS['Partial phase'] } catch { return PRESETS['Partial phase'] } }

/** The lane list of an HDR recording: presets, then one row per lane (name, exposure, optional gain, frames). */
export function LaneEditor({ lanes, onChange }: { lanes: Lane[]; onChange: (l: Lane[]) => void }) {
  const edit = (i: number, k: keyof Lane, v: string) => onChange(lanes.map((x, j) => (j === i ? { ...x, [k]: v } : x)))
  return (
    <div>
      <div className="btnrow tight" role="group" aria-label="Lane presets">
        {Object.keys(PRESETS).map((p) => <button key={p} type="button" className="grow" onClick={() => onChange(PRESETS[p])}>{p}</button>)}
      </div>
      {lanes.map((l, i) => (
        <div className="lane" key={i}>
          <Field label={`Lane ${i + 1} name`}>{(id) => <input id={id} value={l.name} onChange={(e) => edit(i, 'name', e.target.value)} />}</Field>
          <div className="btnrow tight">
            <Field label="Exposure, ms">{(id) => <input id={id} inputMode="decimal" value={l.exposure_ms} onChange={(e) => edit(i, 'exposure_ms', e.target.value)} />}</Field>
            <Field label="Gain (optional)">{(id) => <input id={id} inputMode="decimal" value={l.gain} onChange={(e) => edit(i, 'gain', e.target.value)} />}</Field>
            <Field label="Frames">{(id) => <input id={id} inputMode="numeric" value={l.frames} onChange={(e) => edit(i, 'frames', e.target.value)} />}</Field>
          </div>
          {lanes.length > 2 && <button type="button" onClick={() => onChange(lanes.filter((_, j) => j !== i))}>Remove lane {i + 1}</button>}
        </div>
      ))}
      <div className="btnrow tight">
        <button type="button" disabled={lanes.length >= 6} onClick={() => onChange([...lanes, { name: `lane${lanes.length + 1}`, exposure_ms: '1', gain: '', frames: '100' }])}>Add lane</button>
      </div>
    </div>
  )
}
